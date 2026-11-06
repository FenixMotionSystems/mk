//! Linux accessibility backend over AT-SPI2, spoken directly on its D-Bus (no
//! Python, GTK or libatspi). Same contract as the Windows and macOS backends.
//!
//! Actions go through the app (`Action.DoAction`), so a button can be pressed
//! in a window that is not active and without coordinates, which Wayland would
//! not allow anyway. Keystrokes are different: they land in whichever window the
//! compositor has active, so `ui_type` refuses to type unless the target's
//! window is the active one.
//!
//! On Wayland `Component.GetExtents` is relative to the window, so `x`/`y` of an
//! element are not screen coordinates there. Act by name, never by those pixels.

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, connection::Builder};
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Type};

use super::{MatchMode, UiElement, UiHit, UiState, match_name};
use crate::windows::WindowInfo;

const REGISTRY_BUS: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const NULL_PATH: &str = "/org/a11y/atspi/null";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const COMPONENT: &str = "org.a11y.atspi.Component";
const ACTION: &str = "org.a11y.atspi.Action";
const TEXT: &str = "org.a11y.atspi.Text";
const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const VALUE: &str = "org.a11y.atspi.Value";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

const WINDOW_ID_PREFIX: &str = "atspi:";
/// A hung app must not hang the caller: each call gives up on its own.
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
const WALK_DEADLINE: Duration = Duration::from_secs(20);
const NODE_CAP: usize = 5000;
const DEPTH_CAP: usize = 64;
const FIND_HIT_CAP: usize = 50;

/// Chromium fills rich inputs with this placeholder for embedded objects.
const OBJECT_REPLACEMENT: char = '\u{FFFC}';

const ELECTRON_HINT: &str = "Las apps Electron/Chromium solo aparecen si arrancan con \
    ACCESSIBILITY_ENABLED=1 y --force-renderer-accessibility.";

// AtspiStateType values: positions in the two 32-bit words of `GetState`.
const STATE_ACTIVE: u32 = 1;
const STATE_CHECKED: u32 = 4;
const STATE_EXPANDABLE: u32 = 9;
const STATE_EXPANDED: u32 = 10;
const STATE_ENABLED: u32 = 8;
const STATE_FOCUSED: u32 = 12;
const STATE_PRESSED: u32 = 20;
const STATE_SHOWING: u32 = 25;

/// Pure: read one state bit out of the `au` that `GetState` returns.
fn state_has(words: &[u32], state: u32) -> bool {
    words
        .get((state / 32) as usize)
        .is_some_and(|word| word & (1 << (state % 32)) != 0)
}

/// Pure: AT-SPI role names to the vocabulary the other backends use, so
/// `mk ui tree --role button` means the same on every OS.
fn normalize_role(role: &str) -> String {
    match role {
        "push button" | "toggle button" | "push button menu" => "button".into(),
        "entry" | "password text" => "edit".into(),
        "check box" | "check menu item" => "checkbox".into(),
        "radio button" | "radio menu item" => "radiobutton".into(),
        "combo box" => "combobox".into(),
        "menu item" => "menuitem".into(),
        "document web" => "document".into(),
        other => other.replace(' ', ""),
    }
}

/// Pure: first action whose name matches `wanted`, in `wanted`'s order of
/// preference rather than the order the app happens to list them.
fn pick_action(names: &[String], wanted: &[&str]) -> Option<i32> {
    wanted.iter().find_map(|want| {
        names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(want))
            .map(|index| index as i32)
    })
}

/// Pure: bus names (`:1.23`) contain a colon, so split on the path's slash.
fn parse_window_id(id: &str) -> Option<Node> {
    let rest = id.strip_prefix(WINDOW_ID_PREFIX)?;
    let (bus, path) = rest.split_once(":/")?;
    if bus.is_empty() {
        return None;
    }
    Some(Node {
        bus: bus.to_string(),
        path: format!("/{path}"),
    })
}

fn format_window_id(node: &Node) -> String {
    format!("{WINDOW_ID_PREFIX}{}:{}", node.bus, node.path)
}

fn is_window_role(role: &str) -> bool {
    matches!(role, "frame" | "dialog" | "alert" | "file chooser" | "window")
}

/// Pure: an empty rich input reads as the placeholder alone, which is not text.
fn clean_text(raw: String) -> String {
    raw.chars().filter(|c| *c != OBJECT_REPLACEMENT).collect()
}

/// Pure: prefer a match the user can actually reach, as several controls often
/// share a name and the hidden copy tends to come first.
fn best_match<T>(matches: Vec<(T, bool)>) -> Option<T> {
    let mut first = None;
    for (item, reachable) in matches {
        if reachable {
            return Some(item);
        }
        first.get_or_insert(item);
    }
    first
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    bus: String,
    path: String,
}

struct Bus {
    conn: Connection,
}

impl Bus {
    fn connect() -> Result<Self> {
        let session = Connection::session().context("no hay bus de sesión de D-Bus")?;
        let address: String = session
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                Some("org.a11y.Bus"),
                "GetAddress",
                &(),
            )
            .context("AT-SPI no responde (¿falta at-spi2-core?)")?
            .body()
            .deserialize()
            .context("respuesta inválida de org.a11y.Bus")?;
        let conn = Builder::address(address.as_str())
            .context("dirección del bus de accesibilidad inválida")?
            .method_timeout(CALL_TIMEOUT)
            .build()
            .context("no se pudo conectar al bus de accesibilidad")?;
        Ok(Self { conn })
    }

    fn call<B, R>(&self, node: &Node, iface: &str, method: &str, body: &B) -> Result<R>
    where
        B: serde::Serialize + DynamicType,
        R: DeserializeOwned + Type,
    {
        let reply = self
            .conn
            .call_method(
                Some(node.bus.as_str()),
                node.path.as_str(),
                Some(iface),
                method,
                body,
            )
            .with_context(|| format!("{iface}.{method} falló"))?;
        reply
            .body()
            .deserialize()
            .with_context(|| format!("respuesta inesperada de {iface}.{method}"))
    }

    fn property(&self, node: &Node, iface: &str, name: &str) -> Result<OwnedValue> {
        self.call(node, PROPERTIES, "Get", &(iface, name))
    }

    fn string_property(&self, node: &Node, iface: &str, name: &str) -> Result<String> {
        String::try_from(self.property(node, iface, name)?).map_err(|e| anyhow!("{name}: {e}"))
    }

    fn name(&self, node: &Node) -> String {
        self.string_property(node, ACCESSIBLE, "Name").unwrap_or_default()
    }

    fn role(&self, node: &Node) -> String {
        self.call(node, ACCESSIBLE, "GetRoleName", &()).unwrap_or_default()
    }

    fn states(&self, node: &Node) -> Vec<u32> {
        self.call(node, ACCESSIBLE, "GetState", &()).unwrap_or_default()
    }

    fn children(&self, node: &Node) -> Vec<Node> {
        let kids: Vec<(String, OwnedObjectPath)> =
            self.call(node, ACCESSIBLE, "GetChildren", &()).unwrap_or_default();
        kids.into_iter()
            .map(|(bus, path)| Node { bus, path: path.as_str().to_string() })
            .filter(|kid| kid.path != NULL_PATH && !kid.bus.is_empty())
            .collect()
    }

    fn interfaces(&self, node: &Node) -> Vec<String> {
        self.call(node, ACCESSIBLE, "GetInterfaces", &()).unwrap_or_default()
    }

    fn automation_id(&self, node: &Node) -> String {
        self.string_property(node, ACCESSIBLE, "AccessibleId").unwrap_or_default()
    }

    fn extents(&self, node: &Node) -> (i32, i32, i32, i32) {
        // 0 = ATSPI_COORD_TYPE_SCREEN.
        self.call(node, COMPONENT, "GetExtents", &0u32).unwrap_or((0, 0, 0, 0))
    }

    fn element(&self, node: &Node) -> UiElement {
        let states = self.states(node);
        let (x, y, width, height) = self.extents(node);
        UiElement {
            role: normalize_role(&self.role(node)),
            name: self.name(node),
            x,
            y,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            is_enabled: state_has(&states, STATE_ENABLED),
            is_offscreen: !state_has(&states, STATE_SHOWING),
            automation_id: self.automation_id(node),
        }
    }

    fn action_names(&self, node: &Node) -> Vec<String> {
        // Not `GetActions`: Chromium leaves the names blank in it.
        let count = self
            .property(node, ACTION, "NActions")
            .ok()
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(0);
        (0..count)
            .map(|index| self.call(node, ACTION, "GetName", &index).unwrap_or_default())
            .collect()
    }

    fn do_action(&self, node: &Node, wanted: &[&str]) -> Result<()> {
        let names = self.action_names(node);
        let index = pick_action(&names, wanted).ok_or_else(|| {
            anyhow!("el control no ofrece esa acción (ofrece: {})", names.join(", "))
        })?;
        let done: bool = self.call(node, ACTION, "DoAction", &index)?;
        if !done {
            bail!("la app rechazó la acción '{}'", names[index as usize]);
        }
        Ok(())
    }

    fn grab_focus(&self, node: &Node) -> Result<()> {
        let done: bool = self.call(node, COMPONENT, "GrabFocus", &())?;
        if !done {
            bail!("la app rechazó dar el foco al control");
        }
        Ok(())
    }

    fn text(&self, node: &Node) -> Result<String> {
        let count = i32::try_from(self.property(node, TEXT, "CharacterCount")?)
            .map_err(|e| anyhow!("CharacterCount: {e}"))?;
        let raw: String = self.call(node, TEXT, "GetText", &(0i32, count))?;
        Ok(clean_text(raw))
    }

    fn value(&self, node: &Node) -> Result<f64> {
        f64::try_from(self.property(node, VALUE, "CurrentValue")?)
            .map_err(|e| anyhow!("CurrentValue: {e}"))
    }

    /// Depth-first, bounded: Electron apps can expose thousands of nodes. A
    /// truncated walk says so on stderr instead of quietly missing controls.
    fn walk(&self, root: &Node, visit: &mut dyn FnMut(&Node) -> bool) {
        let deadline = Instant::now() + WALK_DEADLINE;
        let mut stack = vec![(root.clone(), 0usize)];
        let mut seen = 0usize;
        while let Some((node, depth)) = stack.pop() {
            if seen >= NODE_CAP || Instant::now() > deadline {
                eprintln!("mk: árbol recortado tras {seen} elementos");
                return;
            }
            seen += 1;
            if !visit(&node) {
                return;
            }
            if depth < DEPTH_CAP {
                let kids = self.children(&node);
                stack.extend(kids.into_iter().rev().map(|kid| (kid, depth + 1)));
            }
        }
    }

    fn matches_query(&self, node: &Node, query: &str, mode: &MatchMode) -> bool {
        let candidate = if *mode == MatchMode::AutomationId {
            self.automation_id(node)
        } else {
            self.name(node)
        };
        !candidate.is_empty() && match_name(&candidate, query, mode)
    }

    fn top_windows(&self) -> Vec<(Node, String)> {
        let root = Node { bus: REGISTRY_BUS.into(), path: ROOT_PATH.into() };
        let mut windows = Vec::new();
        for app in self.children(&root) {
            let app_name = self.name(&app);
            if app_name == "gnome-shell" {
                continue;
            }
            for window in self.children(&app) {
                if is_window_role(&self.role(&window)) {
                    windows.push((window, app_name.clone()));
                }
            }
        }
        windows
    }

    /// Accepts AT-SPI ids and ids from other backends (matched by title), so
    /// `--window` keeps working with whatever `mk window list` printed.
    fn resolve_window(&self, window_id: &str) -> Result<Node> {
        if let Some(node) = parse_window_id(window_id) {
            // AT-SPI ids die with the window, and a dead one would otherwise
            // read as "control not found".
            if self.role(&node).is_empty() {
                bail!("la ventana {window_id} ya no existe (usa `mk window list`)");
            }
            return Ok(node);
        }
        let info = crate::windows::list_windows()?
            .into_iter()
            .find(|w| w.id == window_id)
            .ok_or_else(|| anyhow!("ventana {window_id} no encontrada (usa `mk window list`)"))?;
        self.top_windows()
            .into_iter()
            .map(|(node, _)| node)
            .find(|node| self.name(node) == info.title)
            .ok_or_else(|| {
                anyhow!("la ventana '{}' no aparece en AT-SPI. {ELECTRON_HINT}", info.title)
            })
    }

    fn find(&self, window_id: &str, query: &str, mode: &MatchMode) -> Result<(Node, Node)> {
        let window = self.resolve_window(window_id)?;
        let mut found = Vec::new();
        self.walk(&window, &mut |node| {
            if self.matches_query(node, query, mode) {
                let states = self.states(node);
                let reachable =
                    state_has(&states, STATE_ENABLED) && state_has(&states, STATE_SHOWING);
                found.push((node.clone(), reachable));
            }
            true
        });
        let hint = if *mode == MatchMode::AutomationId { "--id" } else { "--name" };
        let node = best_match(found).ok_or_else(|| {
            anyhow!(
                "control '{query}' ({hint}) no encontrado en ventana {window_id} \
                 (prueba `mk ui tree --window {window_id}`)"
            )
        })?;
        Ok((window, node))
    }
}

/// On-screen windows as AT-SPI sees them: the only view of native Wayland
/// windows on compositors with no window-listing protocol, such as GNOME.
pub fn list_frames() -> Result<Vec<WindowInfo>> {
    let bus = Bus::connect()?;
    let mut frames = Vec::new();
    for (node, app_name) in bus.top_windows() {
        let states = bus.states(&node);
        if !state_has(&states, STATE_SHOWING) {
            continue;
        }
        let (x, y, width, height) = bus.extents(&node);
        frames.push(WindowInfo {
            id: format_window_id(&node),
            title: bus.name(&node),
            app_name,
            x,
            y,
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            is_active: state_has(&states, STATE_ACTIVE),
            pid: None,
        });
    }
    Ok(frames)
}

/// Name of the deepest focused control in the active window. The enclosing
/// document also reports focus, hence deepest rather than first.
pub fn focused_control_name() -> Result<Option<String>> {
    let bus = Bus::connect()?;
    let Some((window, _)) = bus
        .top_windows()
        .into_iter()
        .find(|(node, _)| state_has(&bus.states(node), STATE_ACTIVE))
    else {
        bail!("ninguna ventana activa en AT-SPI. {ELECTRON_HINT}");
    };
    let mut focused = None;
    bus.walk(&window, &mut |node| {
        if state_has(&bus.states(node), STATE_FOCUSED) {
            focused = Some(bus.name(node));
        }
        true
    });
    Ok(focused)
}

pub fn ui_tree_for_window(window_id: &str) -> Result<Vec<UiElement>> {
    let bus = Bus::connect()?;
    let window = bus.resolve_window(window_id)?;
    let mut out = Vec::new();
    bus.walk(&window, &mut |node| {
        out.push(bus.element(node));
        true
    });
    Ok(out)
}

pub fn ui_click(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    let snapshot = bus.element(&node);
    bus.do_action(&node, &["click", "press", "activate", "jump", "open", "toggle"])?;
    Ok(snapshot)
}

pub fn ui_toggle(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    bus.do_action(&node, &["toggle", "click", "press", "activate"])?;
    Ok(bus.element(&node))
}

pub fn ui_set_value(
    window_id: &str,
    query: &str,
    value: &str,
    mode: &MatchMode,
) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    if !bus.interfaces(&node).iter().any(|iface| iface == EDITABLE_TEXT) {
        bail!(
            "el control '{query}' no admite escribir por el árbol \
             (usa `mk ui type --clipboard`, que exige ventana activa)"
        );
    }
    let done: bool = bus.call(&node, EDITABLE_TEXT, "SetTextContents", &value)?;
    if !done {
        bail!("la app rechazó el texto");
    }
    Ok(bus.element(&node))
}

pub fn ui_double_click(_window_id: &str, query: &str, _mode: &MatchMode) -> Result<UiElement> {
    bail!("AT-SPI no tiene doble clic para '{query}': usa `mk ui click` o coordenadas en pantalla")
}

pub fn ui_right_click(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiElement> {
    ui_show_menu(window_id, query, mode)
}

pub fn ui_show_menu(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    let snapshot = bus.element(&node);
    bus.do_action(&node, &["showContextMenu", "menu", "popup"])?;
    Ok(snapshot)
}

pub fn ui_focus(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    bus.grab_focus(&node)?;
    Ok(bus.element(&node))
}

fn read_state(bus: &Bus, node: &Node) -> UiState {
    let states = bus.states(node);
    let element = bus.element(node);
    let interfaces = bus.interfaces(node);
    let has = |iface: &str| interfaces.iter().any(|name| name == iface);
    let value = if has(VALUE) {
        bus.value(node).ok().map(|number| number.to_string())
    } else if has(TEXT) {
        bus.text(node).ok()
    } else {
        None
    };
    let toggleable = matches!(element.role.as_str(), "checkbox" | "radiobutton");
    let on = state_has(&states, STATE_CHECKED) || state_has(&states, STATE_PRESSED);
    UiState {
        toggle_state: (toggleable || on).then(|| if on { "on" } else { "off" }.to_string()),
        expand_state: state_has(&states, STATE_EXPANDABLE).then(|| {
            if state_has(&states, STATE_EXPANDED) { "expanded" } else { "collapsed" }.to_string()
        }),
        value,
        element,
    }
}

pub fn ui_get_value(window_id: &str, query: &str, mode: &MatchMode) -> Result<UiState> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    Ok(read_state(&bus, &node))
}

pub fn ui_expand(
    window_id: &str,
    query: &str,
    mode: &MatchMode,
    collapse: bool,
) -> Result<UiState> {
    let bus = Bus::connect()?;
    let (_, node) = bus.find(window_id, query, mode)?;
    let states = bus.states(&node);
    let expanded = state_has(&states, STATE_EXPANDED);
    if expanded == collapse {
        bus.do_action(&node, &["expand or contract", "expand", "toggle", "click", "press"])?;
    }
    Ok(read_state(&bus, &node))
}

pub fn ui_wait(
    window_id: &str,
    query: &str,
    mode: &MatchMode,
    timeout: Duration,
    interval: Duration,
    require_visible: bool,
) -> Result<UiElement> {
    let bus = Bus::connect()?;
    super::wait_for_element(query, window_id, timeout, interval, require_visible, || {
        let (_, node) = bus.find(window_id, query, mode)?;
        Ok(bus.element(&node))
    })
}

pub fn ui_shot(
    _window_id: &str,
    query: &str,
    _mode: &MatchMode,
    _out_path: &str,
    _pad: u32,
    _zoom: u32,
) -> Result<UiElement> {
    bail!(
        "`mk ui shot` de '{query}' no está disponible con AT-SPI: en Wayland la posición es \
         relativa a la ventana. Usa `mk screenshot`"
    )
}

pub fn ui_find(query: &str, mode: &MatchMode) -> Result<Vec<UiHit>> {
    let bus = Bus::connect()?;
    let mut hits = Vec::new();
    for (window, _) in bus.top_windows() {
        let window_id = format_window_id(&window);
        let window_title = bus.name(&window);
        bus.walk(&window, &mut |node| {
            if bus.matches_query(node, query, mode) {
                hits.push(UiHit {
                    window_id: window_id.clone(),
                    window_title: window_title.clone(),
                    element: bus.element(node),
                });
            }
            hits.len() < FIND_HIT_CAP
        });
        if hits.len() >= FIND_HIT_CAP {
            break;
        }
    }
    Ok(hits)
}

/// Keys land in the compositor's active window, not in the element, so this
/// refuses to type unless the target's window is the active one.
pub fn ui_type(
    window_id: &str,
    query: &str,
    mode: &MatchMode,
    text: &str,
    via_clipboard: bool,
) -> Result<UiElement> {
    let bus = Bus::connect()?;
    let (window, node) = bus.find(window_id, query, mode)?;
    bus.grab_focus(&node)?;
    if !state_has(&bus.states(&window), STATE_ACTIVE) {
        bail!(
            "no se escribe nada: la ventana de '{query}' no es la activa y las teclas irían a otra"
        );
    }
    if !state_has(&bus.states(&node), STATE_FOCUSED) {
        bail!("no se escribe nada: el control '{query}' no tomó el foco");
    }
    let backend = crate::input::detect_backend()?;
    if via_clipboard {
        crate::output::paste::paste(text, "ctrl+v", backend.as_ref())?;
    } else {
        backend.type_text(text)?;
    }
    Ok(bus.element(&node))
}

pub fn ui_drag(
    _window_id: &str,
    from: &str,
    to: &str,
    _mode: &MatchMode,
) -> Result<(UiElement, UiElement)> {
    bail!("AT-SPI no tiene arrastre de '{from}' a '{to}': usa `mk drag` con coordenadas en pantalla")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_bits_span_both_words() {
        let words = [1 << STATE_ACTIVE | 1 << STATE_SHOWING, 1 << 1];
        assert!(state_has(&words, STATE_ACTIVE));
        assert!(state_has(&words, STATE_SHOWING));
        assert!(!state_has(&words, STATE_FOCUSED));
        assert!(state_has(&words, 33), "33 is bit 1 of the second word");
        assert!(!state_has(&[], STATE_ACTIVE), "a failed read means no state, not a panic");
    }

    #[test]
    fn roles_share_the_other_backends_vocabulary() {
        assert_eq!(normalize_role("push button"), "button");
        assert_eq!(normalize_role("toggle button"), "button");
        assert_eq!(normalize_role("entry"), "edit");
        assert_eq!(normalize_role("check box"), "checkbox");
        assert_eq!(normalize_role("internal frame"), "internalframe");
    }

    #[test]
    fn action_follows_preference_not_app_order_and_ignores_case() {
        let names = vec!["showContextMenu".to_string(), "Press".to_string(), "click".to_string()];
        assert_eq!(pick_action(&names, &["click", "press"]), Some(2));
        assert_eq!(pick_action(&names, &["press"]), Some(1));
        assert_eq!(pick_action(&names, &["toggle"]), None);
    }

    #[test]
    fn window_id_survives_the_colon_in_bus_names() {
        let node = Node {
            bus: ":1.23".into(),
            path: "/org/a11y/atspi/accessible/5".into(),
        };
        assert_eq!(parse_window_id(&format_window_id(&node)), Some(node));
    }

    #[test]
    fn foreign_window_ids_are_not_atspi_ids() {
        assert_eq!(parse_window_id("6291469"), None);
        assert_eq!(parse_window_id("atspi:no-path"), None);
        assert_eq!(parse_window_id("atspi::/x"), None);
    }

    #[test]
    fn empty_rich_input_reads_as_empty() {
        assert_eq!(clean_text("\u{FFFC}".to_string()), "");
        assert_eq!(clean_text("/compact".to_string()), "/compact");
    }

    #[test]
    fn reachable_match_beats_an_earlier_hidden_one() {
        assert_eq!(best_match(vec![("hidden", false), ("shown", true)]), Some("shown"));
        assert_eq!(best_match(vec![("a", false), ("b", false)]), Some("a"));
        assert_eq!(best_match::<&str>(vec![]), None);
    }

    #[test]
    fn window_roles_exclude_regular_controls() {
        assert!(is_window_role("frame"));
        assert!(is_window_role("dialog"));
        assert!(!is_window_role("push button"));
    }
}
