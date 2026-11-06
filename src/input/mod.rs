// Always compiled (not platform-gated) so its unit tests run everywhere.
pub mod scaling;

#[cfg(target_os = "linux")]
pub mod daemon;
#[cfg(target_os = "linux")]
mod layout;
#[cfg(target_os = "linux")]
mod wtype;
#[cfg(target_os = "linux")]
mod xdotool;
#[cfg(target_os = "linux")]
mod ydotool;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

use anyhow::{bail, Result};
#[cfg(target_os = "linux")]
use std::fmt;
#[cfg(target_os = "linux")]
use std::process::Command;

/// Input backends are shared across concurrent macro runs (see
/// `mk::engine`), so they must be safe to use from several threads. All
/// current backends are zero-sized, so this costs nothing.
pub trait InputBackend: Send + Sync {
    fn type_text(&self, text: &str) -> Result<()>;
    fn press_key(&self, key: &str) -> Result<()>;
    fn display_name(&self) -> &str;

    fn mouse_move(&self, _x: i32, _y: i32, _duration_ms: u64) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
    fn mouse_click(&self, _x: i32, _y: i32, _button: &str, _duration_ms: u64) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
    fn mouse_drag(&self, _x1: i32, _y1: i32, _x2: i32, _y2: i32, _duration_ms: u64) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
    fn mouse_down(&self, _button: &str) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
    fn mouse_up(&self, _button: &str) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
    fn mouse_scroll(&self, _clicks: i32, _horizontal: bool) -> Result<()> {
        bail!("Mouse operations are not supported by the {} backend. Please use the daemon backend.", self.display_name())
    }
}

pub use InputBackend as Backend;

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    Wayland,
    X11,
}

#[cfg(target_os = "linux")]
impl fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayServer::Wayland => write!(f, "Wayland"),
            DisplayServer::X11 => write!(f, "X11"),
        }
    }
}

#[cfg(target_os = "linux")]
fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn detect_display_server() -> DisplayServer {
    if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
        if session == "wayland" {
            return DisplayServer::Wayland;
        }
    }
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return DisplayServer::Wayland;
    }
    if std::env::var("DISPLAY").is_ok() {
        return DisplayServer::X11;
    }
    DisplayServer::X11
}

pub fn detect_backend() -> Result<Box<dyn InputBackend>> {
    #[cfg(target_os = "linux")]
    {
        // Prefer daemon if running
        if daemon::daemon_is_running() {
            return Ok(Box::new(daemon::DaemonBackend));
        }

        let server = detect_display_server();
        match server {
            DisplayServer::Wayland => {
                if command_exists("wtype") {
                    return Ok(Box::new(wtype::WtypeBackend));
                }
                if command_exists("ydotool") {
                    return Ok(Box::new(ydotool::YdotoolBackend));
                }
                bail!(
                    "No backend available for Wayland.\n\
                     Options:\n\
                     1. Start mk-daemon:  sudo mk-daemon\n\
                     2. Install wtype:    sudo pacman -S wtype\n\
                     3. Install ydotool:  sudo pacman -S ydotool"
                );
            }
            DisplayServer::X11 => {
                if command_exists("xdotool") {
                    return Ok(Box::new(xdotool::XdotoolBackend));
                }
                if command_exists("wtype") {
                    return Ok(Box::new(wtype::WtypeBackend));
                }
                bail!(
                    "No backend available for X11.\n\
                     Options:\n\
                     1. Start mk-daemon:  sudo mk-daemon\n\
                     2. Install xdotool:  sudo pacman -S xdotool"
                );
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WindowsBackend))
    }

    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::MacosBackend))
    }
}

pub struct DryRunBackend;

impl InputBackend for DryRunBackend {
    fn type_text(&self, text: &str) -> Result<()> {
        println!("[dry-run] type_text: \"{text}\"");
        Ok(())
    }

    fn press_key(&self, key: &str) -> Result<()> {
        println!("[dry-run] press_key: \"{key}\"");
        Ok(())
    }

    fn display_name(&self) -> &str {
        "dry-run"
    }

    fn mouse_move(&self, x: i32, y: i32, duration_ms: u64) -> Result<()> {
        println!("[dry-run] mouse_move: to ({x}, {y}) over {duration_ms}ms");
        Ok(())
    }

    fn mouse_click(&self, x: i32, y: i32, button: &str, duration_ms: u64) -> Result<()> {
        println!("[dry-run] mouse_click: button {button} at ({x}, {y}) over {duration_ms}ms");
        Ok(())
    }

    fn mouse_drag(&self, x1: i32, y1: i32, x2: i32, y2: i32, duration_ms: u64) -> Result<()> {
        println!("[dry-run] mouse_drag: from ({x1}, {y1}) to ({x2}, {y2}) over {duration_ms}ms");
        Ok(())
    }

    fn mouse_down(&self, button: &str) -> Result<()> {
        println!("[dry-run] mouse_down: button {button}");
        Ok(())
    }

    fn mouse_up(&self, button: &str) -> Result<()> {
        println!("[dry-run] mouse_up: button {button}");
        Ok(())
    }

    fn mouse_scroll(&self, clicks: i32, horizontal: bool) -> Result<()> {
        println!("[dry-run] mouse_scroll: clicks {clicks}, horizontal {horizontal}");
        Ok(())
    }
}
