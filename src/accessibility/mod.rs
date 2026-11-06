use anyhow::Result;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiElement {
    pub role: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default)]
    pub is_offscreen: bool,
    /// Stable per-app control identifier (AutomationId). Empty when the app
    /// does not set one. Prefer it over `--name`: it survives relabels and
    /// locales, unlike visible text.
    #[serde(default)]
    pub automation_id: String,
}

/// Readable UI state: the element plus whatever the OS reports through
/// Value / Toggle / ExpandCollapse patterns (each optional — a plain label
/// exposes none of them).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    pub element: UiElement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expand_state: Option<String>,
}

/// One desktop-wide search hit: the element plus its top-level window, so
/// the agent can scope the next action with `--window <window_id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiHit {
    pub window_id: String,
    pub window_title: String,
    pub element: UiElement,
}

/// Shared by the Windows/macOS backends: screenshot `window_id`, crop to the
/// element bounds (+ `pad` px tolerance for shadows/decorations), save to
/// `out_path`. Geometry is best-effort: OS bounds are screen pixels, the
/// capture starts at the window origin reported by `mk window list`.
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn shot_element(
    window_id: &str,
    snap: &UiElement,
    out_path: &str,
    pad: u32,
    zoom: u32,
) -> Result<()> {
    let wins = crate::windows::list_windows()?;
    let win = wins
        .into_iter()
        .find(|w| w.id == window_id)
        .ok_or_else(|| anyhow::anyhow!("Window {window_id} not found"))?;
    let rx = snap.x.saturating_sub(win.x).saturating_sub(pad as i32).max(0) as u32;
    let ry = snap.y.saturating_sub(win.y).saturating_sub(pad as i32).max(0) as u32;
    let rw = snap.width.saturating_add(2 * pad).max(1);
    let rh = snap.height.saturating_add(2 * pad).max(1);
    let tmp = std::env::temp_dir().join(format!("mk-ui-shot-{}.png", std::process::id()));
    let tmp_str = tmp.to_string_lossy().into_owned();
    crate::vision::capture_window(
        window_id,
        &tmp_str,
        crate::vision::ScreenshotFormat::Raw,
        100,
    )
    .context("capture de ventana falló (¿minimizada?)")?;
    crate::vision::crop_image_file(&tmp_str, out_path, (rx, ry, rw, rh), zoom, 90)
        .context("crop al control falló (¿geometría obsoleta? re-lee `mk window list`)")?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

/// Shared wait loop: poll `find` until it yields an element (and, with
/// `require_visible`, one that is enabled and on-screen) or `timeout`.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub(crate) fn wait_for_element(
    query: &str,
    window_id: &str,
    timeout: std::time::Duration,
    interval: std::time::Duration,
    require_visible: bool,
    mut find: impl FnMut() -> Result<UiElement>,
) -> Result<UiElement> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(snapshot) = find() {
            if !require_visible || (snapshot.is_enabled && !snapshot.is_offscreen) {
                return Ok(snapshot);
            }
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Timeout esperando control '{query}' en ventana {window_id} tras {}s",
                timeout.as_secs()
            );
        }
        std::thread::sleep(interval);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Exact,
    Contains,
    Regex,
    /// Exact match on `automation_id` (case-sensitive, no trim): stable ids.
    AutomationId,
}

/// Exact = trim + case-insensitive. Contains = substring case-insensitive.
/// Regex = crate `regex`, case-sensitive salvo `(?i)`; inválida → false.
/// AutomationId = igualdad exacta (los ids son estables y case-sensitive).
pub fn match_name(candidate: &str, query: &str, mode: &MatchMode) -> bool {
    match mode {
        MatchMode::Exact => candidate.trim().to_lowercase() == query.trim().to_lowercase(),
        MatchMode::Contains => candidate.to_lowercase().contains(&query.to_lowercase()),
        MatchMode::Regex => regex::Regex::new(query).map(|re| re.is_match(candidate)).unwrap_or(false),
        MatchMode::AutomationId => candidate == query,
    }
}

pub fn get_ui_tree() -> Result<Vec<UiElement>> {
    // Accessibility stubs - returns an empty list for now.
    // Ready for OS Accessibility API native integrations (UIA, AXUIElement, AT-SPI2).
    Ok(Vec::new())
}

pub fn find_button(name: &str) -> Result<Option<UiElement>> {
    let tree = get_ui_tree()?;
    Ok(tree.into_iter().find(|el| {
        el.role.to_lowercase() == "button" && match_name(&el.name, name, &MatchMode::Exact)
    }))
}

pub fn find_input(placeholder: &str) -> Result<Option<UiElement>> {
    let tree = get_ui_tree()?;
    Ok(tree.into_iter().find(|el| {
        let role = el.role.to_lowercase();
        (role == "input" || role == "text_input" || role == "edit") && match_name(&el.name, placeholder, &MatchMode::Exact)
    }))
}

#[cfg(target_os = "windows")]
pub mod windows_uia;
#[cfg(any(target_os = "macos", test))]
pub mod macos_ax;

#[cfg(target_os = "windows")]
pub use windows_uia::{
    ui_click, ui_double_click, ui_drag, ui_expand, ui_find, ui_focus, ui_get_value,
    ui_right_click, ui_set_value, ui_shot, ui_show_menu, ui_toggle, ui_tree_for_window,
    ui_type, ui_wait,
};

#[cfg(target_os = "macos")]
pub use macos_ax::{
    ui_click, ui_double_click, ui_drag, ui_expand, ui_find, ui_focus, ui_get_value,
    ui_right_click, ui_set_value, ui_shot, ui_show_menu, ui_toggle, ui_tree_for_window,
    ui_type, ui_wait,
};

#[cfg(target_os = "linux")]
pub mod linux_atspi;

#[cfg(target_os = "linux")]
pub use linux_atspi::{
    ui_click, ui_double_click, ui_drag, ui_expand, ui_find, ui_focus, ui_get_value,
    ui_right_click, ui_set_value, ui_shot, ui_show_menu, ui_toggle, ui_tree_for_window,
    ui_type, ui_wait,
};

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_tree_for_window(_window_id: &str) -> Result<Vec<UiElement>> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_click(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_toggle(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_set_value(
    _window_id: &str,
    _query: &str,
    _value: &str,
    _mode: &MatchMode,
) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_double_click(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_right_click(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_focus(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_get_value(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiState> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_expand(
    _window_id: &str,
    _query: &str,
    _mode: &MatchMode,
    _collapse: bool,
) -> Result<UiState> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_wait(
    _window_id: &str,
    _query: &str,
    _mode: &MatchMode,
    _timeout: std::time::Duration,
    _interval: std::time::Duration,
    _require_visible: bool,
) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_shot(
    _window_id: &str,
    _query: &str,
    _mode: &MatchMode,
    _out_path: &str,
    _pad: u32,
    _zoom: u32,
) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_find(_query: &str, _mode: &MatchMode) -> Result<Vec<UiHit>> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_type(
    _window_id: &str,
    _query: &str,
    _mode: &MatchMode,
    _text: &str,
    _via_clipboard: bool,
) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_show_menu(_window_id: &str, _query: &str, _mode: &MatchMode) -> Result<UiElement> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn ui_drag(
    _window_id: &str,
    _from: &str,
    _to: &str,
    _mode: &MatchMode,
) -> Result<(UiElement, UiElement)> {
    anyhow::bail!("UI Automation solo disponible en Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_stubs() {
        let tree = get_ui_tree();
        assert!(tree.is_ok());
        let btn = find_button("Save");
        assert!(btn.is_ok());
        let input = find_input("Username");
        assert!(input.is_ok());
    }

    #[test]
    fn test_match_name_exact_is_trim_case_insensitive() {
        assert!(match_name("Mezclador", "mezclador", &MatchMode::Exact));
        assert!(match_name("  Mezclador  ", "MEZCLADOR", &MatchMode::Exact));
        assert!(!match_name("Mezclador panel", "mezclador", &MatchMode::Exact));
    }

    #[test]
    fn test_match_name_contains_and_regex() {
        assert!(match_name("Delay: relativamente sencillo", "delay", &MatchMode::Contains));
        assert!(!match_name("Delay", "lay$", &MatchMode::Contains));
        assert!(match_name("Track 01", r"^Track \d+$", &MatchMode::Regex));
        assert!(!match_name("Track AB", r"^Track \d+$", &MatchMode::Regex));
        assert!(!match_name("cualquier cosa", "(unclosed", &MatchMode::Regex), "regex inválida = false, no panic");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_uia_module_loads() {
        let automation = crate::accessibility::windows_uia::automation();
        assert!(automation.is_ok(), "UIAutomation::new() debe inicializar COM");
    }

    #[test]
    fn test_automation_id_match_and_compat() {
        // Old JSON without automation_id still deserializes (default "").
        let el: UiElement = serde_json::from_value(serde_json::json!({
            "role": "button", "name": "OK", "x": 1, "y": 2, "width": 3, "height": 4
        }))
        .unwrap();
        assert_eq!(el.automation_id, "");
        assert!(match_name("btn_ok", "btn_ok", &MatchMode::AutomationId));
        assert!(!match_name("btn_ok", "BTN_OK", &MatchMode::AutomationId));
    }

    #[test]
    fn test_ui_hit_serializes() {
        let hit = UiHit {
            window_id: "123".into(),
            window_title: "App".into(),
            element: UiElement {
                role: "button".into(),
                name: "OK".into(),
                x: 1,
                y: 2,
                width: 3,
                height: 4,
                is_enabled: true,
                is_offscreen: false,
                automation_id: "btn_ok".into(),
            },
        };
        let v = serde_json::to_value(&hit).unwrap();
        assert_eq!(v["window_id"], "123");
        assert_eq!(v["element"]["automation_id"], "btn_ok");
    }
}
