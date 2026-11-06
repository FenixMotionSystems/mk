use super::Backend;
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

const SOCKET_PATH: &str = "/tmp/mk-daemon.sock";

/// Physical screen resolution, i.e. the pixel dimensions of what `mk
/// screenshot` captures — the space users read click coordinates from.
///
/// This matters under fractional display scaling. On Wayland, xcap's
/// `Monitor::width()/height()` report the LOGICAL size (e.g. 1728×1152 at
/// 1.667× scale), while xcap's *screenshots* are PHYSICAL pixels (2880×1920).
/// `scale_coords` maps a coordinate into the virtual absolute-pointer range
/// (0..32767) across this resolution; if it used the logical size, every
/// click would land off by the scale factor (≈1.667×) toward the bottom-right
/// — empirically confirmed on GNOME/Wayland (a click aimed at "File" opened
/// "Edit"). Recover the physical extent as `logical × scale_factor` so clicks
/// land where the screenshot shows. `scale_factor` is 1.0 when unscaled, so
/// this is a no-op on non-scaled displays.
///
/// NOTE: verified on Linux/Wayland (1.667× scale). The `× scale_factor`
/// recovery assumes `width()` is logical; validate on Windows/macOS before
/// relying on it there (see docs/computer-use-skill.md).
fn get_screen_resolution() -> (i32, i32) {
    if let Ok(monitors) = xcap::Monitor::all() {
        if let Some(m) = monitors.first() {
            if let (Ok(w), Ok(h)) = (m.width(), m.height()) {
                let scale = m.scale_factor().unwrap_or(1.0);
                let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
                let pw = (w as f32 * scale).round() as i32;
                let ph = (h as f32 * scale).round() as i32;
                return (pw.max(1), ph.max(1));
            }
        }
    }
    (1920, 1080)
}

fn scale_coords(x: i32, y: i32) -> (i32, i32) {
    let (w, h) = get_screen_resolution();
    let scaled_x = (x as i64 * 32767 / w as i64) as i32;
    let scaled_y = (y as i64 * 32767 / h as i64) as i32;
    (scaled_x.clamp(0, 32767), scaled_y.clamp(0, 32767))
}

pub struct DaemonBackend;

impl Backend for DaemonBackend {
    fn type_text(&self, text: &str) -> Result<()> {
        if let Some(strokes) = super::layout::plan(text)? {
            for stroke in &strokes {
                self.press_key(stroke)?;
            }
            return Ok(());
        }
        let mut stream = connect()?;
        // Escape backslashes and newlines: the protocol uses \n as message
        // delimiter, so embedded newlines must be sent as the two-char sequence \n.
        let escaped: String = text.chars().flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c => vec![c],
        }).collect();
        send_command(&mut stream, &format!("TYPE:{escaped}"))?;
        Ok(())
    }

    fn press_key(&self, key: &str) -> Result<()> {
        let mut stream = connect()?;
        send_command(&mut stream, &format!("KEY:{key}"))?;
        Ok(())
    }

    fn mouse_move(&self, x: i32, y: i32, duration_ms: u64) -> Result<()> {
        let (sx, sy) = scale_coords(x, y);
        let mut stream = connect()?;
        if duration_ms > 0 {
            send_command(&mut stream, &format!("MOVE_SMOOTH:{sx}:{sy}:{duration_ms}"))?;
        } else {
            send_command(&mut stream, &format!("MOVE:{sx}:{sy}"))?;
        }
        Ok(())
    }

    fn mouse_click(&self, x: i32, y: i32, button: &str, duration_ms: u64) -> Result<()> {
        self.mouse_move(x, y, duration_ms)?;
        let mut stream = connect()?;
        send_command(&mut stream, &format!("CLICK:{button}"))?;
        Ok(())
    }

    fn mouse_drag(&self, x1: i32, y1: i32, x2: i32, y2: i32, duration_ms: u64) -> Result<()> {
        self.mouse_move(x1, y1, 0)?;
        {
            let mut stream = connect()?;
            send_command(&mut stream, "MOUSE_DOWN:left")?;
        }
        self.mouse_move(x2, y2, duration_ms)?;
        {
            let mut stream = connect()?;
            send_command(&mut stream, "MOUSE_UP:left")?;
        }
        Ok(())
    }

    fn mouse_down(&self, button: &str) -> Result<()> {
        let mut stream = connect()?;
        send_command(&mut stream, &format!("MOUSE_DOWN:{button}"))?;
        Ok(())
    }

    fn mouse_up(&self, button: &str) -> Result<()> {
        let mut stream = connect()?;
        send_command(&mut stream, &format!("MOUSE_UP:{button}"))?;
        Ok(())
    }

    fn mouse_scroll(&self, clicks: i32, horizontal: bool) -> Result<()> {
        let mut stream = connect()?;
        send_command(&mut stream, &format!("SCROLL:{clicks}:{horizontal}"))?;
        Ok(())
    }

    fn display_name(&self) -> &str {
        "mk-daemon"
    }
}

fn connect() -> Result<UnixStream> {
    if !Path::new(SOCKET_PATH).exists() {
        bail!(
            "mk-daemon is not running.\n\
             Start it with: sudo mk-daemon\n\
             Or run: sudo mk daemon start"
        );
    }
    UnixStream::connect(SOCKET_PATH).context("Failed to connect to mk-daemon socket")
}

fn send_command(stream: &mut UnixStream, cmd: &str) -> Result<String> {
    stream
        .write_all(format!("{cmd}\n").as_bytes())
        .context("Failed to send command to daemon")?;

    // The daemon keeps the connection open for further commands, so read a
    // single response line instead of reading to EOF (which would deadlock:
    // the client waiting for a close the daemon never sends).
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .context("Failed to read daemon response")?;

    let response = response.trim();
    if response.starts_with("ERR:") {
        bail!("Daemon error: {}", &response[4..]);
    }

    Ok(response.to_string())
}

pub fn daemon_is_running() -> bool {
    UnixStream::connect(SOCKET_PATH).is_ok()
}

pub fn ping_daemon() -> Result<()> {
    let mut stream = connect()?;
    let response = send_command(&mut stream, "PING")?;
    if response == "OK" {
        Ok(())
    } else {
        bail!("Unexpected daemon response: {response}")
    }
}

/// Daemon protocol version (`VERSION` command, `OK:<n>`). Daemons older than
/// the VERSION command answer `ERR:unknown command`, which surfaces here as
/// an error — callers should report that as "legacy/unknown", not crash.
pub fn daemon_version() -> Result<String> {
    let mut stream = connect()?;
    let response = send_command(&mut stream, "VERSION")?;
    match response.strip_prefix("OK:") {
        Some(v) => Ok(v.to_string()),
        None => bail!("Unexpected daemon response: {response}"),
    }
}
