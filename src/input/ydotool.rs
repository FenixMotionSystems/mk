use super::Backend;
use anyhow::{Context, Result};
use std::process::Command;

pub struct YdotoolBackend;

impl Backend for YdotoolBackend {
    fn type_text(&self, text: &str) -> Result<()> {
        if let Some(strokes) = super::layout::plan(text)? {
            for stroke in &strokes {
                self.press_key(stroke)?;
            }
            return Ok(());
        }
        let status = Command::new("ydotool")
            .args(["type", "--", text])
            .status()
            .context("Failed to run ydotool")?;
        if !status.success() {
            anyhow::bail!("ydotool exited with status: {status}");
        }
        Ok(())
    }

    fn press_key(&self, key: &str) -> Result<()> {
        if key.contains('+') {
            press_combo(key)
        } else {
            let keycode = resolve_key_code_str(key)
                .map(|s| s.to_string())
                .unwrap_or_else(|| key.to_string());
            let status = Command::new("ydotool")
                .args(["key", &keycode])
                .status()
                .context("Failed to run ydotool")?;
            if !status.success() {
                anyhow::bail!("ydotool exited with status: {status}");
            }
            Ok(())
        }
    }

    fn display_name(&self) -> &str {
        "ydotool"
    }
}

fn is_modifier(key: &str) -> bool {
    matches!(
        key.to_lowercase().as_str(),
        "ctrl" | "control" | "alt" | "altgr" | "shift" | "super" | "win" | "meta"
    )
}

fn resolve_modifier_code(key: &str) -> Option<&'static str> {
    match key.to_lowercase().as_str() {
        "ctrl" | "control" => Some("29"),
        "alt" | "meta" => Some("56"),
        "altgr" => Some("100"),
        "shift" => Some("42"),
        "super" | "win" | "logo" => Some("125"),
        _ => None,
    }
}

fn resolve_key_code_str(key: &str) -> Option<&'static str> {
    match key.to_lowercase().as_str() {
        "enter" | "return" => Some("28"),
        "esc" | "escape" => Some("1"),
        "tab" => Some("15"),
        "backspace" => Some("14"),
        "delete" | "del" => Some("111"),
        "up" => Some("103"),
        "down" => Some("108"),
        "left" => Some("105"),
        "right" => Some("106"),
        "space" => Some("57"),
        // digits (evdev: 1=>2 … 9=>10, 0=>11)
        "1" => Some("2"),
        "2" => Some("3"),
        "3" => Some("4"),
        "4" => Some("5"),
        "5" => Some("6"),
        "6" => Some("7"),
        "7" => Some("8"),
        "8" => Some("9"),
        "9" => Some("10"),
        "0" => Some("11"),
        // function keys (evdev: F1=>59 … F10=>68, F11=>87, F12=>88)
        "f1" => Some("59"),
        "f2" => Some("60"),
        "f3" => Some("61"),
        "f4" => Some("62"),
        "f5" => Some("63"),
        "f6" => Some("64"),
        "f7" => Some("65"),
        "f8" => Some("66"),
        "f9" => Some("67"),
        "f10" => Some("68"),
        "f11" => Some("87"),
        "f12" => Some("88"),
        // punctuation (evdev codes, see mk-daemon-linux.rs)
        "-" | "minus" => Some("12"),
        "=" | "equal" => Some("13"),
        "[" | "leftbrace" => Some("26"),
        "]" | "rightbrace" => Some("27"),
        ";" | "semicolon" => Some("39"),
        "'" | "apostrophe" => Some("40"),
        "`" | "grave" => Some("41"),
        "\\" | "backslash" => Some("43"),
        "," | "comma" => Some("51"),
        "." | "dot" | "period" => Some("52"),
        "/" | "slash" => Some("53"),
        "102nd" => Some("86"),
        // letters
        "a" => Some("30"),
        "b" => Some("48"),
        "c" => Some("46"),
        "d" => Some("32"),
        "e" => Some("18"),
        "f" => Some("33"),
        "g" => Some("34"),
        "h" => Some("35"),
        "i" => Some("23"),
        "j" => Some("36"),
        "k" => Some("37"),
        "l" => Some("38"),
        "m" => Some("50"),
        "n" => Some("49"),
        "o" => Some("24"),
        "p" => Some("25"),
        "q" => Some("16"),
        "r" => Some("19"),
        "s" => Some("31"),
        "t" => Some("20"),
        "u" => Some("22"),
        "v" => Some("47"),
        "w" => Some("17"),
        "x" => Some("45"),
        "y" => Some("21"),
        "z" => Some("44"),
        _ => None,
    }
}

fn press_combo(combo: &str) -> Result<()> {
    let parts: Vec<&str> = combo.split('+').collect();
    let mut modifiers = Vec::new();
    let mut main_key = None;

    for part in parts {
        let part = part.trim();
        if is_modifier(part) {
            modifiers.push(part);
        } else if main_key.is_some() {
            // Never silently drop a second main key ("a+b" used to lose "a").
            anyhow::bail!("Chord takes one main key: {combo}");
        } else {
            main_key = Some(part);
        }
    }

    // Resolve everything up front: unknown names bail instead of vanishing
    // mid-sequence (previously "ctrl+1" pressed ctrl alone and returned OK).
    let mut mod_codes = Vec::new();
    for &m in &modifiers {
        match resolve_modifier_code(m) {
            Some(code) => mod_codes.push(code),
            None => anyhow::bail!("Unknown modifier: {m}"),
        }
    }
    let main_code = match main_key {
        Some(key) => Some(
            resolve_key_code_str(key)
                .ok_or_else(|| anyhow::anyhow!("Unknown key: {key}"))?,
        ),
        None => None,
    };
    if mod_codes.is_empty() && main_code.is_none() {
        anyhow::bail!("Empty key combo: {combo}");
    }

    let mut args = Vec::new();
    // Press modifiers
    for code in &mod_codes {
        args.push(format!("{code}:1"));
    }
    // Press and release main key
    if let Some(code) = main_code {
        args.push(format!("{code}:1"));
        args.push(format!("{code}:0"));
    }
    // Release modifiers in reverse order
    for code in mod_codes.iter().rev() {
        args.push(format!("{code}:0"));
    }

    let status = Command::new("ydotool")
        .arg("key")
        .args(args)
        .status()
        .context("Failed to run ydotool")?;

    if !status.success() {
        anyhow::bail!("ydotool exited with status: {status}");
    }
    Ok(())
}
