use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use xcap::Window;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

impl WindowInfo {
    /// Center point of the window — the natural target for a `mk click` that
    /// wants to focus/raise this window by clicking its body.
    pub fn center(&self) -> (i32, i32) {
        (
            self.x + (self.width as i32) / 2,
            self.y + (self.height as i32) / 2,
        )
    }
}

// ── Platform-specific focus (raising a window to the foreground) ────────────

#[cfg(target_os = "windows")]
mod os_impl {
    use anyhow::{bail, Result};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetForegroundWindow, ShowWindow, SetWindowPos, PostMessageW,
        BringWindowToTop,
        SW_RESTORE, SW_MINIMIZE, SW_MAXIMIZE, WM_CLOSE,
        SWP_NOSIZE, SWP_NOMOVE, SWP_NOZORDER, SWP_NOACTIVATE,
        IsIconic,
    };

    fn parse_hwnd(window_id: &str) -> Result<windows_sys::Win32::Foundation::HWND> {
        let hwnd_val: usize = window_id
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid window ID: {e}"))?;
        Ok(hwnd_val as windows_sys::Win32::Foundation::HWND)
    }

    pub fn focus_window(window_id: &str) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        
        unsafe {
            // Only restore if minimized - don't change window state otherwise
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            
            // Bring window to top of Z-order
            BringWindowToTop(hwnd);
            
            // Try SetForegroundWindow
            SetForegroundWindow(hwnd);
            
            Ok(())
        }
    }

    pub fn move_window(window_id: &str, x: i32, y: i32) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            if SetWindowPos(hwnd, 0, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) == 0 {
                bail!("Failed to move window");
            }
            Ok(())
        }
    }

    pub fn resize_window(window_id: &str, width: u32, height: u32) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            if SetWindowPos(hwnd, 0, 0, 0, width as i32, height as i32, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) == 0 {
                bail!("Failed to resize window");
            }
            Ok(())
        }
    }

    pub fn minimize_window(window_id: &str) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            ShowWindow(hwnd, SW_MINIMIZE);
            Ok(())
        }
    }

    pub fn maximize_window(window_id: &str) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            ShowWindow(hwnd, SW_MAXIMIZE);
            Ok(())
        }
    }

    pub fn restore_window(window_id: &str) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            Ok(())
        }
    }

    pub fn close_window(window_id: &str) -> Result<()> {
        let hwnd = parse_hwnd(window_id)?;
        unsafe {
            if PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
                bail!("Failed to post WM_CLOSE message to window");
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
mod os_impl {
    use anyhow::{bail, Result};
    use std::process::Command;

    /// True when the current Linux session is Wayland (as opposed to X11).
    fn is_wayland() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v.eq_ignore_ascii_case("wayland"))
                .unwrap_or(false)
    }

    fn check_wayland_unsupported(op_name: &str) -> Result<()> {
        if is_wayland() {
            bail!(
                "{op_name}-by-id is not supported on Wayland: the compositor owns \
                 window management and exposes no generic protocol for it. \
                 Use input simulation or a compositor backend (hyprctl/swaymsg/kwin). \
                 See docs/window-control.md."
            );
        }
        Ok(())
    }

    /// Run `hyprctl` with prebuilt args; hyper-explicit errors (never silent:
    /// a renamed dispatcher between Hyprland versions must shout, not vanish).
    fn run_hyprctl(args: &[String]) -> Result<()> {
        let status = Command::new("hyprctl")
            .args(args)
            .status()
            .map_err(|e| anyhow::anyhow!("hyprctl failed to run: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            bail!("hyprctl {} failed", args.join(" "))
        }
    }

    fn run_swaymsg(args: &[String]) -> Result<()> {
        let status = Command::new("swaymsg")
            .args(args)
            .status()
            .map_err(|e| anyhow::anyhow!("swaymsg failed to run: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            bail!("swaymsg {} failed", args.join(" "))
        }
    }

    /// Pure command builders live next to the parsers below (ungated so their
    /// tests run on every OS); the runners above just execute the argv.

    pub fn focus_window(window_id: &str) -> Result<()> {
        // Compositor first: on Hyprland/Sway this reaches native Wayland
        // windows that xdotool/xcb cannot see at all.
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return run_hyprctl(&super::hyprctl_focus_cmd(window_id));
        }
        if std::env::var("SWAYSOCK").is_ok() {
            return run_swaymsg(&super::swaymsg_focus_cmd(window_id));
        }
        check_wayland_unsupported("focus")?;
        let status = Command::new("xdotool")
            .arg("windowactivate")
            .arg(window_id)
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("xdotool windowactivate failed for id {window_id}"),
            Err(_) => bail!(
                "cannot focus window on X11: xdotool not found. Install it, or \
                 use input simulation. See docs/window-control.md."
            ),
        }
    }

    pub fn move_window(window_id: &str, x: i32, y: i32) -> Result<()> {
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return run_hyprctl(&super::hyprctl_move_cmd(window_id, x, y));
        }
        if std::env::var("SWAYSOCK").is_ok() {
            return run_swaymsg(&super::swaymsg_move_cmd(window_id, x, y));
        }
        check_wayland_unsupported("move")?;
        let status = Command::new("xdotool")
            .args(["windowmove", window_id, &x.to_string(), &y.to_string()])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("xdotool windowmove failed for id {window_id}"),
            Err(_) => bail!("cannot move window on X11: xdotool not found."),
        }
    }

    pub fn resize_window(window_id: &str, width: u32, height: u32) -> Result<()> {
        if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
            return run_hyprctl(&super::hyprctl_resize_cmd(window_id, width, height));
        }
        if std::env::var("SWAYSOCK").is_ok() {
            return run_swaymsg(&super::swaymsg_resize_cmd(window_id, width, height));
        }
        check_wayland_unsupported("resize")?;
        let status = Command::new("xdotool")
            .args(["windowsize", window_id, &width.to_string(), &height.to_string()])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("xdotool windowsize failed for id {window_id}"),
            Err(_) => bail!("cannot resize window on X11: xdotool not found."),
        }
    }

    pub fn minimize_window(window_id: &str) -> Result<()> {
        check_wayland_unsupported("minimize")?;
        let status = Command::new("xdotool")
            .args(["windowminimize", window_id])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("xdotool windowminimize failed for id {window_id}"),
            Err(_) => bail!("cannot minimize window on X11: xdotool not found."),
        }
    }

    pub fn maximize_window(window_id: &str) -> Result<()> {
        check_wayland_unsupported("maximize")?;
        let status = Command::new("wmctrl")
            .args(["-ir", window_id, "-b", "add,maximized_vert,maximized_horz"])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("wmctrl maximize failed for id {window_id}"),
            Err(_) => bail!("cannot maximize window on X11: wmctrl not found."),
        }
    }

    pub fn restore_window(window_id: &str) -> Result<()> {
        check_wayland_unsupported("restore")?;
        let status = Command::new("wmctrl")
            .args(["-ir", window_id, "-b", "remove,maximized_vert,maximized_horz"])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("wmctrl restore failed for id {window_id}"),
            Err(_) => bail!("cannot restore window on X11: wmctrl not found."),
        }
    }

    pub fn close_window(window_id: &str) -> Result<()> {
        check_wayland_unsupported("close")?;
        let status = Command::new("xdotool")
            .args(["windowclose", window_id])
            .status();
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => bail!("xdotool windowclose failed for id {window_id}"),
            Err(_) => bail!("cannot close window on X11: xdotool not found."),
        }
    }
}

#[cfg(target_os = "macos")]
mod os_impl {
    use anyhow::{bail, Result};
    use std::process::Command;

    fn run_applescript(script: &str) -> Result<()> {
        let status = Command::new("osascript").args(["-e", script]).status()?;
        if status.success() {
            Ok(())
        } else {
            bail!("AppleScript execution failed")
        }
    }

    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    pub fn focus_window(app_name: &str, title: &str) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             set frontmost to true\n\
             try\n\
             perform action \"AXRaise\" of (first window whose name is \"{title_esc}\")\n\
             on error\n\
             try\n\
             perform action \"AXRaise\" of (first window whose name contains \"{title_esc}\")\n\
             on error\n\
             perform action \"AXRaise\" of window 1\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell\n\
             tell application \"{app}\" to activate"
        );
        run_applescript(&script)
    }

    pub fn move_window(app_name: &str, title: &str, x: i32, y: i32) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             try\n\
             set position of (first window whose name is \"{title_esc}\") to {{ {x}, {y} }}\n\
             on error\n\
             try\n\
             set position of (first window whose name contains \"{title_esc}\") to {{ {x}, {y} }}\n\
             on error\n\
             set position of window 1 to {{ {x}, {y} }}\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell"
        );
        run_applescript(&script)
    }

    pub fn resize_window(app_name: &str, title: &str, width: u32, height: u32) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             try\n\
             set size of (first window whose name is \"{title_esc}\") to {{ {width}, {height} }}\n\
             on error\n\
             try\n\
             set size of (first window whose name contains \"{title_esc}\") to {{ {width}, {height} }}\n\
             on error\n\
             set size of window 1 to {{ {width}, {height} }}\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell"
        );
        run_applescript(&script)
    }

    pub fn minimize_window(app_name: &str, title: &str) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             try\n\
             set value of attribute \"AXMinimized\" of (first window whose name is \"{title_esc}\") to true\n\
             on error\n\
             try\n\
             set value of attribute \"AXMinimized\" of (first window whose name contains \"{title_esc}\") to true\n\
             on error\n\
             set value of attribute \"AXMinimized\" of window 1 to true\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell"
        );
        run_applescript(&script)
    }

    pub fn maximize_window(app_name: &str, title: &str) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             try\n\
             set value of attribute \"AXZoomed\" of (first window whose name is \"{title_esc}\") to true\n\
             on error\n\
             try\n\
             set value of attribute \"AXZoomed\" of (first window whose name contains \"{title_esc}\") to true\n\
             on error\n\
             set value of attribute \"AXZoomed\" of window 1 to true\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell"
        );
        run_applescript(&script)
    }

    pub fn restore_window(app_name: &str, title: &str) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"System Events\"\n\
             tell process \"{app}\"\n\
             try\n\
             set value of attribute \"AXMinimized\" of (first window whose name is \"{title_esc}\") to false\n\
             on error\n\
             try\n\
             set value of attribute \"AXMinimized\" of (first window whose name contains \"{title_esc}\") to false\n\
             on error\n\
             set value of attribute \"AXMinimized\" of window 1 to false\n\
             end try\n\
             end try\n\
             try\n\
             set value of attribute \"AXZoomed\" of (first window whose name is \"{title_esc}\") to false\n\
             on error\n\
             try\n\
             set value of attribute \"AXZoomed\" of (first window whose name contains \"{title_esc}\") to false\n\
             on error\n\
             set value of attribute \"AXZoomed\" of window 1 to false\n\
             end try\n\
             end try\n\
             end tell\n\
             end tell"
        );
        run_applescript(&script)
    }

    pub fn close_window(app_name: &str, title: &str) -> Result<()> {
        let app = escape(app_name);
        let title_esc = escape(title);
        let script = format!(
            "tell application \"{app}\"\n\
             try\n\
             close (first window whose name is \"{title_esc}\")\n\
             on error\n\
             try\n\
             close (first window whose name contains \"{title_esc}\")\n\
             on error\n\
             close window 1\n\
             end try\n\
             end try\n\
             end tell"
        );
        run_applescript(&script)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod os_impl {
    use anyhow::{bail, Result};
    pub fn focus_window(_window_id: &str) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn move_window(_window_id: &str, _x: i32, _y: i32) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn resize_window(_window_id: &str, _width: u32, _height: u32) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn minimize_window(_window_id: &str) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn maximize_window(_window_id: &str) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn restore_window(_window_id: &str) -> Result<()> {
        bail!("Platform not supported")
    }
    pub fn close_window(_window_id: &str) -> Result<()> {
        bail!("Platform not supported")
    }
}

#[cfg(target_os = "windows")]
fn pid_for_window(w: &xcap::Window) -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let id: usize = w.id().ok()? as usize;
    let hwnd = id as windows_sys::Win32::Foundation::HWND;
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    if pid == 0 { None } else { Some(pid) }
}

#[cfg(not(target_os = "windows"))]
fn pid_for_window(_w: &xcap::Window) -> Option<u32> {
    None
}

/// Opt-in compositor backends (Phase 4): Hyprland (`hyprctl`) and Sway/i3
/// (`swaymsg`) expose real Wayland window lists where xcb sees nothing.
///
/// Design: pure `parse_*` functions (unit-tested with sample JSON, runnable
/// on any OS) + thin gated runners that shell out. `list_windows` tries
/// `compositor_windows()` first on Linux and falls back to xcap, so a
/// missing/broken binary degrades to today's behavior instead of erroring.
#[cfg(target_os = "linux")]
fn compositor_windows() -> Result<Vec<WindowInfo>> {
    if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        if let Ok(list) = hyprland_windows() {
            return Ok(list);
        }
    }
    if std::env::var("SWAYSOCK").is_ok() {
        if let Ok(list) = sway_windows() {
            return Ok(list);
        }
    }
    anyhow::bail!("no compositor backend available")
}

#[cfg(target_os = "linux")]
fn hyprland_windows() -> Result<Vec<WindowInfo>> {
    let out = std::process::Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .map_err(|e| anyhow::anyhow!("hyprctl failed: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("hyprctl clients -j failed");
    }
    parse_hyprctl_clients(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(target_os = "linux")]
fn sway_windows() -> Result<Vec<WindowInfo>> {
    let out = std::process::Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .map_err(|e| anyhow::anyhow!("swaymsg failed: {e}"))?;
    if !out.status.success() {
        anyhow::bail!("swaymsg -t get_tree failed");
    }
    parse_sway_tree(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `hyprctl clients -j` (array of {address,at:[x,y],size:[w,h],
/// title,class,focused,pid}). Missing/invalid entries are skipped.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_hyprctl_clients(json: &str) -> Result<Vec<WindowInfo>> {
    let clients: serde_json::Value =
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("hyprctl JSON: {e}"))?;
    let mut list = Vec::new();
    for c in clients.as_array().cloned().unwrap_or_default() {
        let at = c.get("at").and_then(|v| v.as_array());
        let size = c.get("size").and_then(|v| v.as_array());
        let (Some(at), Some(size)) = (at, size) else {
            continue;
        };
        let (Some(x), Some(y)) = (at.first().and_then(|v| v.as_i64()), at.get(1).and_then(|v| v.as_i64())) else {
            continue;
        };
        let (Some(w), Some(h)) = (size.first().and_then(|v| v.as_u64()), size.get(1).and_then(|v| v.as_u64())) else {
            continue;
        };
        list.push(WindowInfo {
            id: c.get("address").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
            title: c.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            app_name: c.get("class").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            x: x as i32,
            y: y as i32,
            width: w as u32,
            height: h as u32,
            is_active: c.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
            pid: c.get("pid").and_then(|v| v.as_u64()).map(|p| p as u32).filter(|p| *p != 0),
        });
    }
    Ok(list)
}

/// A sway tree node is a *view* (real window) when it carries `app_id`
/// (native Wayland) or `window_properties` (XWayland).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn sway_is_view(node: &serde_json::Value) -> bool {
    node.get("app_id").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty())
        || node.get("window_properties").is_some()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn sway_collect(node: &serde_json::Value, out: &mut Vec<WindowInfo>) {
    if sway_is_view(node) {
        if let Some(rect) = node.get("rect") {
            let x = rect.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = rect.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let w = rect.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let h = rect.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if w > 0 && h > 0 {
                let app = node
                    .get("app_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        node.get("window_properties")
                            .and_then(|p| p.get("class"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default();
                out.push(WindowInfo {
                    id: node.get("id").map(|v| v.to_string()).unwrap_or_else(|| "?".into()),
                    title: node.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    app_name: app,
                    x,
                    y,
                    width: w,
                    height: h,
                    is_active: node.get("focused").and_then(|v| v.as_bool()).unwrap_or(false),
                    pid: node.get("pid").and_then(|v| v.as_i64()).filter(|p| *p > 0).map(|p| p as u32),
                });
            }
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                sway_collect(child, out);
            }
        }
    }
}

/// Parse `swaymsg -t get_tree` (nested nodes/floating_nodes) into flat views.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_sway_tree(json: &str) -> Result<Vec<WindowInfo>> {
    let tree: serde_json::Value =
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("sway JSON: {e}"))?;
    let mut out = Vec::new();
    sway_collect(&tree, &mut out);
    Ok(out)
}

/// Exact hyprctl/swaymsg argv in one place, unit-tested verbatim so a
/// compositor version drift is caught by reading the test.
/// NOTE (tiled layouts): pixel move/resize is best-effort — a tiling layout
/// may ignore or reabsorb it. Guaranteed only on floating windows.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hyprctl_focus_cmd(id: &str) -> Vec<String> {
    vec!["dispatch".into(), "focuswindow".into(), format!("address:{id}")]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hyprctl_move_cmd(id: &str, x: i32, y: i32) -> Vec<String> {
    vec![
        "dispatch".into(),
        "movewindowpixel".into(),
        "exact".into(),
        format!("{x}"),
        format!("{y},address:{id}"),
    ]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn hyprctl_resize_cmd(id: &str, w: u32, h: u32) -> Vec<String> {
    vec![
        "dispatch".into(),
        "resizewindowpixel".into(),
        "exact".into(),
        format!("{w}"),
        format!("{h},address:{id}"),
    ]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn swaymsg_focus_cmd(id: &str) -> Vec<String> {
    vec![format!("[con_id=\"{id}\"]"), "focus".into()]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn swaymsg_move_cmd(id: &str, x: i32, y: i32) -> Vec<String> {
    vec![format!("[con_id=\"{id}\"]"), "move".into(), "position".into(), format!("{x} {y}")]
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn swaymsg_resize_cmd(id: &str, w: u32, h: u32) -> Vec<String> {
    // Canonical i3/sway form (swaymsg joins argv with spaces first).
    vec![
        format!("[con_id=\"{id}\"]"),
        "resize".into(),
        "set".into(),
        "width".into(),
        format!("{w}px"),
        "height".into(),
        format!("{h}px"),
    ]
}

/// Enumerate all on-screen windows with geometry and focused state.
pub fn list_windows() -> Result<Vec<WindowInfo>> {
    // On Linux, a compositor backend (Hyprland/Sway) sees native Wayland
    // windows that xcb/XWayland cannot. Try it first; fall through to xcap.
    #[cfg(target_os = "linux")]
    {
        if let Ok(list) = compositor_windows() {
            return Ok(list);
        }
    }

    let windows = Window::all().map_err(|e| anyhow::anyhow!("Failed to list windows: {e}"))?;

    let mut list = Vec::new();
    for w in windows {
        let (Ok(w_id), Ok(w_title), Ok(w_app_name)) = (w.id(), w.title(), w.app_name()) else {
            continue;
        };
        let (Ok(w_x), Ok(w_y), Ok(w_width), Ok(w_height)) =
            (w.x(), w.y(), w.width(), w.height())
        else {
            continue;
        };
        // Native focused-state; degrade to false if the backend can't tell.
        let is_active = w.is_focused().unwrap_or(false);

        list.push(WindowInfo {
            id: w_id.to_string(),
            title: w_title,
            app_name: w_app_name,
            x: w_x,
            y: w_y,
            width: w_width,
            height: w_height,
            is_active,
            pid: pid_for_window(&w),
        });
    }

    // GNOME on Wayland exposes no window list: AT-SPI is the only view of its
    // native windows. Not fatal without it, since it may simply not be running.
    #[cfg(target_os = "linux")]
    if let Ok(frames) = crate::accessibility::linux_atspi::list_frames() {
        list = merge_frames(list, frames);
    }

    Ok(list)
}

/// Add AT-SPI windows to the list. A window seen by both keeps its existing id,
/// which window operations understand, and takes AT-SPI's word on being active
/// because xcap cannot tell on Wayland.
#[cfg(any(target_os = "linux", test))]
fn merge_frames(mut list: Vec<WindowInfo>, frames: Vec<WindowInfo>) -> Vec<WindowInfo> {
    for frame in frames {
        match list.iter_mut().find(|w| w.title == frame.title) {
            Some(existing) => existing.is_active |= frame.is_active,
            None => list.push(frame),
        }
    }
    list
}

/// Poll `list_fn` until a window matches `title` or `timeout` expires.
/// `exact=false` = substring case-insensitive; `exact=true` = trim + case-insensitive equality.
pub fn wait_for_window_with(
    title: &str,
    exact: bool,
    timeout: std::time::Duration,
    interval: std::time::Duration,
    mut list_fn: impl FnMut() -> anyhow::Result<Vec<WindowInfo>>,
) -> anyhow::Result<WindowInfo> {
    let deadline = std::time::Instant::now() + timeout;
    let query = title.trim().to_lowercase();
    loop {
        let list = list_fn().unwrap_or_default();
        let hit = list.into_iter().find(|w| {
            if exact {
                w.title.trim().to_lowercase() == query
            } else {
                w.title.to_lowercase().contains(&query)
            }
        });
        if let Some(w) = hit {
            return Ok(w);
        }
        if std::time::Instant::now() >= deadline {
            anyhow::bail!("Timeout esperando ventana con título '{title}' tras {}s", timeout.as_secs());
        }
        std::thread::sleep(interval);
    }
}

pub fn wait_for_window(title: &str, exact: bool, timeout: std::time::Duration, interval: std::time::Duration) -> anyhow::Result<WindowInfo> {
    wait_for_window_with(title, exact, timeout, interval, list_windows)
}

/// The currently focused window, if any backend can report it.
pub fn active_window() -> Result<WindowInfo> {
    let list = list_windows()?;
    list.into_iter()
        .find(|w| w.is_active)
        .context("No active window reported (backend may not expose focus on this session)")
}

/// Helper to find a window by its ID and return a clone of WindowInfo.
#[allow(dead_code)]
fn find_window(window_id: &str) -> Result<WindowInfo> {
    let list = list_windows()?;
    list.into_iter()
        .find(|w| w.id == window_id)
        .ok_or_else(|| anyhow::anyhow!("Window with ID {window_id} not found"))
}

/// Raise the window with the given id to the foreground (best-effort, per-OS).
pub fn focus_window(window_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::focus_window(&w.app_name, &w.title)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::focus_window(window_id)
    }
}

/// Move the window to the specified coordinates.
pub fn move_window(window_id: &str, x: i32, y: i32) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::move_window(&w.app_name, &w.title, x, y)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::move_window(window_id, x, y)
    }
}

/// Resize the window to the specified width and height.
pub fn resize_window(window_id: &str, width: u32, height: u32) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::resize_window(&w.app_name, &w.title, width, height)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::resize_window(window_id, width, height)
    }
}

/// Minimize the window.
pub fn minimize_window(window_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::minimize_window(&w.app_name, &w.title)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::minimize_window(window_id)
    }
}

/// Maximize the window.
pub fn maximize_window(window_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::maximize_window(&w.app_name, &w.title)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::maximize_window(window_id)
    }
}

/// Restore the window from a minimized or maximized state.
pub fn restore_window(window_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::restore_window(&w.app_name, &w.title)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::restore_window(window_id)
    }
}

/// Close the window.
pub fn close_window(window_id: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let w = find_window(window_id)?;
        os_impl::close_window(&w.app_name, &w.title)
    }

    #[cfg(not(target_os = "macos"))]
    {
        os_impl::close_window(window_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_windows_runs() {
        // Without a display server (headless CI) Linux has nothing to list,
        // and erroring is the honest answer there.
        #[cfg(target_os = "linux")]
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let list = list_windows();
        assert!(list.is_ok());
    }

    #[test]
    fn test_window_info_pid_serializes() {
        let w = WindowInfo {
            id: "123".into(),
            title: "t".into(),
            app_name: "a".into(),
            x: 0, y: 0, width: 100, height: 100,
            is_active: false,
            pid: Some(4242),
        };
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["pid"], 4242);

        let w2 = WindowInfo { pid: None, ..w };
        let v2 = serde_json::to_value(&w2).unwrap();
        assert!(v2.get("pid").is_none());
    }

    #[test]
    fn test_wait_for_window_timeout() {
        use std::time::Duration;
        let start = std::time::Instant::now();
        let res = wait_for_window_with(
            "ventana-que-no-existe-xyz",
            false,
            Duration::from_millis(120),
            Duration::from_millis(30),
            || Ok(vec![]),
        );
        assert!(res.is_err());
        assert!(start.elapsed() >= Duration::from_millis(100));
        assert!(res.unwrap_err().to_string().contains("Timeout"));
    }

    #[test]
    fn test_wait_for_window_finds_on_retry() {
        use std::cell::Cell;
        use std::time::Duration;
        let calls = Cell::new(0);
        let res = wait_for_window_with("Code", false, Duration::from_secs(2), Duration::from_millis(10), || {
            calls.set(calls.get() + 1);
            if calls.get() < 3 {
                Ok(vec![])
            } else {
                Ok(vec![WindowInfo {
                    id: "1".into(), title: "Visual Studio Code".into(), app_name: "Code".into(),
                    x: 0, y: 0, width: 800, height: 600, is_active: true, pid: None,
                }])
            }
        });
        assert!(res.is_ok());
        assert_eq!(res.unwrap().id, "1");
    }

    #[test]
    fn test_center_is_midpoint() {
        let w = WindowInfo {
            id: "1".into(),
            title: "t".into(),
            app_name: "a".into(),
            x: 100,
            y: 200,
            width: 400,
            height: 300,
            is_active: false,
            pid: None,
        };
        assert_eq!(w.center(), (300, 350));
    }

    #[test]
    fn test_parse_hyprctl_clients() {
        let json = r#"[
            {"address":"0xabc","at":[100,200],"size":[800,600],"title":"Terminal","class":"kitty","focused":true,"pid":1234},
            {"address":"0xdef","at":[0,0],"size":[1920,1080],"title":"Code","class":"Code","focused":false,"pid":5678},
            {"broken":true}
        ]"#;
        let list = parse_hyprctl_clients(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "0xabc");
        assert_eq!(list[0].title, "Terminal");
        assert_eq!(list[0].app_name, "kitty");
        assert_eq!((list[0].x, list[0].y), (100, 200));
        assert_eq!((list[0].width, list[0].height), (800, 600));
        assert!(list[0].is_active);
        assert_eq!(list[0].pid, Some(1234));
        assert!(!list[1].is_active);
        assert!(parse_hyprctl_clients("not json").is_err());
    }

    #[test]
    fn test_parse_sway_tree() {
        let json = r#"{
            "nodes": [
                {"name": "ws1", "rect": {"x":0,"y":0,"width":1920,"height":1080}, "focused": false,
                 "nodes": [
                    {"id": 11, "name": "Terminal", "app_id": "kitty",
                     "rect": {"x":100,"y":200,"width":800,"height":600},
                     "focused": true, "pid": 1234, "nodes": [], "floating_nodes": []},
                    {"name": "split", "rect": {"x":0,"y":0,"width":10,"height":10},
                     "nodes": [], "floating_nodes": []}
                 ], "floating_nodes": []}
            ],
            "floating_nodes": [
                {"id": 22, "name": "Dialog", "window_properties": {"class": "Zenity", "title": "Dialog"},
                 "rect": {"x":10,"y":10,"width":200,"height":100},
                 "focused": false, "pid": 999, "nodes": [], "floating_nodes": []}
            ]
        }"#;
        let list = parse_sway_tree(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].app_name, "kitty");
        assert_eq!(list[0].pid, Some(1234));
        assert!(list[0].is_active);
        assert_eq!(list[1].app_name, "Zenity");
        assert_eq!((list[1].width, list[1].height), (200, 100));
        assert!(parse_sway_tree("{bad").is_err());
    }

    #[test]
    fn merge_frames_adds_unseen_windows_and_keeps_known_ids() {
        let window = |id: &str, title: &str, is_active: bool| WindowInfo {
            id: id.into(),
            title: title.into(),
            app_name: "app".into(),
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            is_active,
            pid: None,
        };
        let merged = merge_frames(
            vec![window("1", "Widgets", false)],
            vec![window("atspi:a", "Widgets", true), window("atspi:b", "IDE", false)],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "1", "a known window keeps the id window ops understand");
        assert!(merged[0].is_active, "AT-SPI's active state wins over xcap's blind false");
        assert_eq!(merged[1].id, "atspi:b");
    }

    #[test]
    fn test_compositor_cmd_shapes() {
        assert_eq!(
            hyprctl_focus_cmd("0xabc"),
            vec!["dispatch", "focuswindow", "address:0xabc"]
        );
        assert_eq!(
            hyprctl_move_cmd("0xabc", 100, -20),
            vec!["dispatch", "movewindowpixel", "exact", "100", "-20,address:0xabc"]
        );
        assert_eq!(
            hyprctl_resize_cmd("0xabc", 800, 600),
            vec!["dispatch", "resizewindowpixel", "exact", "800", "600,address:0xabc"]
        );
        assert_eq!(swaymsg_focus_cmd("11"), vec!["[con_id=\"11\"]", "focus"]);
        assert_eq!(
            swaymsg_move_cmd("11", 100, 200),
            vec!["[con_id=\"11\"]", "move", "position", "100 200"]
        );
        assert_eq!(
            swaymsg_resize_cmd("11", 800, 600),
            vec!["[con_id=\"11\"]", "resize", "set", "width", "800px", "height", "600px"]
        );
    }
}
