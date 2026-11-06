//! Keyboard-layout awareness for backends that send physical key codes.
//!
//! A virtual keyboard sends positions, not characters: the compositor applies
//! the user's layout, so on a Spanish layout the US `/` key produces `-`. This
//! asks the session which layout is active, reads its keymap through
//! libxkbcommon (loaded at runtime, so it is not a hard dependency) and turns
//! text into the key presses that really produce each character.
//!
//! Characters that need a dead key (`á` on Spanish) have no single key press, so
//! the plan fails before typing anything rather than leaving half a text.

use anyhow::{Result, bail};
use std::collections::HashMap;
use std::ffi::CString;
use std::process::Command;
use std::sync::OnceLock;
use xkbcommon_dl::{
    xkb_context_flags, xkb_keymap_compile_flags, xkb_keysym_t, xkb_rule_names, xkbcommon_option,
};

/// Evdev codes and the names both the daemon and ydotool accept for them.
const KEY_NAMES: &[(u32, &str)] = &[
    (2, "1"), (3, "2"), (4, "3"), (5, "4"), (6, "5"), (7, "6"), (8, "7"), (9, "8"), (10, "9"),
    (11, "0"), (12, "minus"), (13, "equal"), (16, "q"), (17, "w"), (18, "e"), (19, "r"), (20, "t"),
    (21, "y"), (22, "u"), (23, "i"), (24, "o"), (25, "p"), (26, "leftbrace"), (27, "rightbrace"),
    (30, "a"), (31, "s"), (32, "d"), (33, "f"), (34, "g"), (35, "h"), (36, "j"), (37, "k"),
    (38, "l"), (39, "semicolon"), (40, "apostrophe"), (41, "grave"), (43, "backslash"), (44, "z"),
    (45, "x"), (46, "c"), (47, "v"), (48, "b"), (49, "n"), (50, "m"), (51, "comma"), (52, "dot"),
    (53, "slash"), (57, "space"), (86, "102nd"),
];

/// XKB's own offset from evdev codes, fixed by its keycode definition.
const XKB_EVDEV_OFFSET: u32 = 8;
/// Levels 1-4 of a standard key: plain, Shift, AltGr, AltGr+Shift.
const LEVEL_MODIFIERS: [&str; 4] = ["", "shift+", "altgr+", "altgr+shift+"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutName {
    layout: String,
    variant: String,
}

/// Pure: first plain keyboard layout in a GNOME input-sources value, such as
/// `[('ibus', 'typing-booster'), ('xkb', 'es+nodeadkeys')]`.
fn parse_gnome_sources(raw: &str) -> Option<LayoutName> {
    let (_, rest) = raw.split_once("('xkb', '")?;
    let (name, _) = rest.split_once('\'')?;
    let (layout, variant) = name.split_once('+').unwrap_or((name, ""));
    Some(LayoutName { layout: layout.into(), variant: variant.into() })
}

/// Pure: the layout line of `localectl status`.
fn parse_localectl(raw: &str) -> Option<LayoutName> {
    let line = raw.lines().find(|line| line.trim_start().starts_with("X11 Layout:"))?;
    let (_, value) = line.split_once(':')?;
    let layout = value.trim().split(',').next()?.trim();
    (!layout.is_empty()).then(|| LayoutName { layout: layout.into(), variant: String::new() })
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn detect_layout() -> Option<LayoutName> {
    for key in ["mru-sources", "sources"] {
        if let Some(raw) = command_output("gsettings", &["get", "org.gnome.desktop.input-sources", key])
            && let Some(name) = parse_gnome_sources(&raw)
        {
            return Some(name);
        }
    }
    parse_localectl(&command_output("localectl", &["status"])?)
}

/// Pure: lowest level wins, so `a` is typed plain even if AltGr also yields it.
fn build_table(entries: impl IntoIterator<Item = (u32, usize, char)>) -> HashMap<char, String> {
    let mut best: HashMap<char, (usize, String)> = HashMap::new();
    for (code, level, character) in entries {
        let Some((_, name)) = KEY_NAMES.iter().find(|(known, _)| *known == code) else {
            continue;
        };
        let Some(modifiers) = LEVEL_MODIFIERS.get(level) else {
            continue;
        };
        if best.get(&character).is_none_or(|(known, _)| level < *known) {
            best.insert(character, (level, format!("{modifiers}{name}")));
        }
    }
    best.into_iter().map(|(character, (_, combo))| (character, combo)).collect()
}

/// Pure: the key presses for `text`, or the first character the layout lacks.
fn plan_text(text: &str, table: &HashMap<char, String>) -> Result<Vec<String>, char> {
    text.chars()
        .map(|character| match character {
            '\n' => Ok("enter".to_string()),
            '\t' => Ok("tab".to_string()),
            other => table.get(&other).cloned().ok_or(other),
        })
        .collect()
}

fn read_keymap(name: &LayoutName) -> Option<HashMap<char, String>> {
    let xkb = xkbcommon_option()?;
    let layout = CString::new(name.layout.as_str()).ok()?;
    let variant = CString::new(name.variant.as_str()).ok()?;
    let names = xkb_rule_names {
        rules: std::ptr::null(),
        model: std::ptr::null(),
        layout: layout.as_ptr(),
        variant: variant.as_ptr(),
        options: std::ptr::null(),
    };
    let mut entries = Vec::new();
    // SAFETY: `names` and its strings outlive the compile call, the keymap and
    // context are only used before their unref, and `syms` is read within the
    // count libxkbcommon returned while the keymap is alive.
    unsafe {
        let context = (xkb.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS);
        if context.is_null() {
            return None;
        }
        let keymap = (xkb.xkb_keymap_new_from_names)(
            context,
            &names,
            xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        );
        if !keymap.is_null() {
            for &(code, _) in KEY_NAMES {
                let keycode = code + XKB_EVDEV_OFFSET;
                let levels = (xkb.xkb_keymap_num_levels_for_key)(keymap, keycode, 0);
                for level in 0..levels.min(LEVEL_MODIFIERS.len() as u32) {
                    let mut syms: *const xkb_keysym_t = std::ptr::null();
                    let count = (xkb.xkb_keymap_key_get_syms_by_level)(
                        keymap, keycode, 0, level, &mut syms,
                    );
                    if count != 1 || syms.is_null() {
                        continue;
                    }
                    // Dead keys have no code point, so they convert to 0.
                    let point = (xkb.xkb_keysym_to_utf32)(*syms);
                    if let Some(character) = char::from_u32(point).filter(|c| *c != '\0') {
                        entries.push((code, level as usize, character));
                    }
                }
            }
            (xkb.xkb_keymap_unref)(keymap);
        }
        (xkb.xkb_context_unref)(context);
    }
    (!entries.is_empty()).then(|| build_table(entries))
}

fn active_table() -> Option<&'static HashMap<char, String>> {
    static TABLE: OnceLock<Option<HashMap<char, String>>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let name = detect_layout()?;
            // US is what the backends already type natively.
            if name.layout == "us" && name.variant.is_empty() {
                return None;
            }
            read_keymap(&name)
        })
        .as_ref()
}

/// Key presses that type `text` on the active layout. `None` means the layout
/// is US or unknown, so the backend's native typing applies.
pub fn plan(text: &str) -> Result<Option<Vec<String>>> {
    let Some(table) = active_table() else {
        return Ok(None);
    };
    match plan_text(text, table) {
        Ok(strokes) => Ok(Some(strokes)),
        Err(missing) => bail!(
            "'{missing}' no se puede escribir con una sola pulsación en este teclado y no se ha \
             escrito nada: usa `mk paste`"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_sources_skip_input_methods_and_split_variants() {
        let raw = "[('ibus', 'typing-booster'), ('xkb', 'es')]\n";
        assert_eq!(
            parse_gnome_sources(raw),
            Some(LayoutName { layout: "es".into(), variant: String::new() })
        );
        assert_eq!(
            parse_gnome_sources("[('xkb', 'es+nodeadkeys')]"),
            Some(LayoutName { layout: "es".into(), variant: "nodeadkeys".into() })
        );
        assert_eq!(parse_gnome_sources("@a(ss) []"), None);
    }

    #[test]
    fn localectl_takes_the_first_layout() {
        let raw = "   System Locale: LANG=es_ES.UTF-8\n      X11 Layout: es,us\n";
        assert_eq!(
            parse_localectl(raw),
            Some(LayoutName { layout: "es".into(), variant: String::new() })
        );
        assert_eq!(parse_localectl("      X11 Layout: \n"), None);
        assert_eq!(parse_localectl("nothing here"), None);
    }

    #[test]
    fn lowest_level_wins_and_unknown_keys_are_ignored() {
        let table = build_table([(8, 2, 'x'), (45, 0, 'x'), (8, 1, '/'), (999, 0, 'q')]);
        assert_eq!(table.get(&'x').map(String::as_str), Some("x"));
        assert_eq!(table.get(&'/').map(String::as_str), Some("shift+7"));
        assert_eq!(table.get(&'q'), None);
    }

    #[test]
    fn plan_uses_the_table_and_stops_at_the_first_missing_character() {
        let table = build_table([(8, 1, '/'), (46, 0, 'c')]);
        assert_eq!(
            plan_text("/c\n\t", &table),
            Ok(vec!["shift+7".into(), "c".into(), "enter".into(), "tab".into()])
        );
        assert_eq!(plan_text("/á", &table), Err('á'));
    }

    /// Reads the real Spanish keymap where libxkbcommon and its data exist.
    #[test]
    fn real_spanish_keymap_maps_what_broke_compact() {
        let name = LayoutName { layout: "es".into(), variant: String::new() };
        let Some(table) = read_keymap(&name) else {
            eprintln!("skipped: libxkbcommon or its layout data is not available");
            return;
        };
        let combo = |c: char| table.get(&c).map(String::as_str);
        assert_eq!(combo('/'), Some("shift+7"));
        assert_eq!(combo('-'), Some("slash"));
        assert_eq!(combo('a'), Some("a"));
        assert_eq!(combo('A'), Some("shift+a"));
        assert_eq!(combo('ñ'), Some("semicolon"));
        assert_eq!(combo('@'), Some("altgr+2"));
        assert_eq!(combo('<'), Some("102nd"));
        assert_eq!(combo('á'), None, "needs a dead key");
    }
}
