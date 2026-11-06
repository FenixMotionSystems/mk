use mk::input;
use mk::doctor;
use mk::parser;
use mk::scheduler;


use anyhow::{bail, Result};
#[cfg(target_os = "linux")]
use anyhow::Context;
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use mk::input::{Backend, DryRunBackend};
use mk::parser::{Interpreter, Logger};

#[derive(Parser)]
#[command(
    name = "mk",
    about = "Automate keyboard and mouse input",
    long_about = "Automate keyboard and mouse input on Linux, Windows, and macOS.\n\nPlatform-specific backends:\n  • Linux:   xdotool (X11), ydotool (Wayland), libinput (daemon)\n  • Windows: Win32 API (native)\n  • macOS:   AppleScript + CoreGraphics\n\nRecipe: window list (fresco) → actuar con --focus o mk ui (sin foco) → screenshot -w --raw → leer",
    version
)]
struct Cli {
    /// Print actions without executing them
    #[arg(long, global = true)]
    dry_run: bool,

    /// Log actions to a file with timestamps
    #[arg(short, long, global = true)]
    log: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Type a text message
    Text {
        /// The message to type
        message: String,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Press Enter
    Enter {
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Press a key combination
    Key {
        /// Key to press, e.g. "ctrl+s", "enter", "alt+tab"
        key: String,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Wait for a duration
    Wait {
        /// Duration: "10s", "5m", "2h", "250ms"
        duration: String,
    },
    /// Copy text to clipboard and paste
    Paste {
        /// Text to paste
        text: String,
        /// Shortcut key combination to trigger paste (default: ctrl+v)
        #[arg(short, long, default_value = "ctrl+v")]
        shortcut: String,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Execute an action after a delay
    In {
        /// Duration to wait: "10s", "5m", "250ms"
        duration: String,
        #[command(subcommand)]
        action: ScheduledAction,
    },
    /// Execute an action at a specific time (HH:MM local)
    At {
        /// Time in HH:MM format
        time: String,
        #[command(subcommand)]
        action: ScheduledAction,
    },
    /// Keep the session awake by pressing a key periodically
    KeepAwake {
        /// Interval: "4m", "30s" (default: 4m)
        #[arg(default_value = "4m")]
        interval: String,
        /// Key to press (default: F15)
        #[arg(short, long, default_value = "F15")]
        key: String,
    },
    /// Run a script file
    Run {
        /// Path to the .mk script file
        file: String,
    },
    /// Copy file content to clipboard and paste with formatting
    PasteFile {
        /// Path to the file
        path: String,
    },
    /// Copy directory contents recursively to clipboard and paste with formatting
    PasteDir {
        /// Path to the directory
        path: String,
    },
    /// Move mouse cursor to coordinates
    Move {
        /// Destination X coordinate (pixels)
        x: i32,
        /// Destination Y coordinate (pixels)
        y: i32,
        /// Duration of progressive slide (e.g. "500ms")
        #[arg(short, long)]
        duration: Option<String>,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Click a mouse button at coordinates
    Click {
        /// X coordinate (pixels)
        x: i32,
        /// Y coordinate (pixels)
        y: i32,
        /// Button to click: left, right, middle
        #[arg(short, long, default_value = "left")]
        button: String,
        /// Duration of progressive slide before clicking (e.g. "500ms")
        #[arg(short, long)]
        duration: Option<String>,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Drag the mouse from start to end coordinates
    Drag {
        /// Start X coordinate
        x1: i32,
        /// Start Y coordinate
        y1: i32,
        /// End X coordinate
        x2: i32,
        /// End Y coordinate
        y2: i32,
        /// Duration of slide (e.g. "500ms", default "500ms")
        #[arg(short, long)]
        duration: Option<String>,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Press and hold a mouse button
    MouseDown {
        /// Button to press: left, right, middle (default: left)
        #[arg(default_value = "left")]
        button: String,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Release a mouse button
    MouseUp {
        /// Button to release: left, right, middle (default: left)
        #[arg(default_value = "left")]
        button: String,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Scroll the mouse wheel
    Scroll {
        /// Number of scroll clicks (negative for down/left, positive for up/right)
        #[arg(allow_hyphen_values = true)]
        clicks: i32,
        /// Scroll horizontally instead of vertically
        #[arg(long)]
        horizontal: bool,
        /// Focus window id in-process before acting (avoids focus-revert between invocations)
        #[arg(long)]
        focus: Option<String>,
    },
    /// Print current mouse cursor position (x, y)
    MousePos,
    /// Take a screenshot
    Screenshot {
        /// Path to save the image
        path: String,
        /// Window ID to capture (from `mk window list`). If omitted, captures a monitor.
        #[arg(short, long)]
        window: Option<String>,
        /// Monitor index to capture (0=primary, 1=secondary, etc). Default: 0
        #[arg(short, long)]
        monitor: Option<usize>,
        /// Raw mode: full resolution PNG. Default: compressed JPEG (smaller)
        #[arg(long)]
        raw: bool,
        /// JPEG quality 1-100 (default: 85). Only applies in compressed mode.
        #[arg(long, default_value_t = 85)]
        quality: u8,
        /// Draw red crosshair at current cursor position
        #[arg(long)]
        cursor: bool,
        /// Crop region x,y,w,h applied after capture (e.g. "100,200,800,600")
        #[arg(long)]
        crop: Option<String>,
        /// Zoom factor 1-8 applied after crop (default: 1)
        #[arg(long, default_value_t = 1)]
        zoom: u32,
        /// Window title substring to capture (resolved immediately, no wait).
        /// Conflicts with --window.
        #[arg(long, conflicts_with = "window")]
        title: Option<String>,
    },
    /// Manage the mk-daemon service
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Check system dependencies and display diagnostics
    Doctor,
    /// Is it safe to act? Prints JSON, exits 1 if any condition fails or is unknown
    Check {
        /// Minimum time since the last keyboard or mouse input: "30m", "90s"
        #[arg(long)]
        idle: Option<String>,
        /// The active window's title must contain this (case-insensitive)
        #[arg(long)]
        active_window: Option<String>,
        /// The focused control's name must contain this (case-insensitive)
        #[arg(long)]
        focused: Option<String>,
    },
    /// List, inspect, or focus on-screen windows
    Window {
        #[command(subcommand)]
        action: WindowAction,
    },
    /// Semantic UI control via OS accessibility APIs (Windows UIA first)
    Ui {
        #[command(subcommand)]
        action: UiAction,
    },
    /// Read the system clipboard (e.g. after select-all + copy in an app)
    Clipboard {
        #[command(subcommand)]
        action: ClipboardAction,
    },
    /// List monitors as JSON (index, name, geometry) for multi-monitor targeting
    Monitors,
    /// Post-process saved images without re-capturing (crop/zoom, dimensions)
    Vision {
        #[command(subcommand)]
        action: VisionAction,
    },
}

#[derive(Subcommand)]
enum WindowAction {
    /// List all on-screen windows (JSON): id, title, app, geometry, is_active
    List,
    /// Print the currently focused window as JSON
    Active,
    /// Raise a window to the foreground (best-effort, per-OS)
    Focus {
        /// Window id (as reported by `mk window list`)
        id: Option<String>,
        /// ...or title substring (resolved immediately, no wait)
        #[arg(long)]
        title: Option<String>,
    },
    /// Move a window to coordinates x, y (best-effort, per-OS)
    Move {
        /// Window id
        id: String,
        /// Target X coordinate
        x: i32,
        /// Target Y coordinate
        y: i32,
    },
    /// Resize a window to width x height (best-effort, per-OS)
    Resize {
        /// Window id
        id: String,
        /// Target width
        width: u32,
        /// Target height
        height: u32,
    },
    /// Minimize a window (best-effort, per-OS)
    Minimize {
        /// Window id
        id: String,
    },
    /// Maximize a window (best-effort, per-OS)
    Maximize {
        /// Window id
        id: String,
    },
    /// Restore a window from minimized/maximized state (best-effort, per-OS)
    Restore {
        /// Window id
        id: String,
    },
    /// Close a window (best-effort, per-OS)
    Close {
        /// Window id
        id: String,
    },
    /// Wait until a window appears (polls list internally)
    Wait {
        /// Substring to match against window title (use --exact for full match)
        #[arg(long)]
        title: String,
        /// Exact match instead of substring
        #[arg(long, default_value_t = false)]
        exact: bool,
        /// How long to wait, e.g. "10s" (default "10s")
        #[arg(long, default_value = "10s")]
        timeout: String,
        /// Poll interval, e.g. "400ms" (default "400ms")
        #[arg(long, default_value = "400ms")]
        interval: String,
    },
    /// Cycle focus with Alt+Tab (input simulation; the Wayland-safe switcher)
    AltTab {
        /// How many times to press it (default: 1)
        #[arg(default_value_t = 1)]
        count: u32,
    },
}

#[derive(Subcommand)]
enum UiAction {
    /// List UI elements of a window as JSON (name-based targeting)
    Tree {
        /// Window id (from `mk window list`; default: active window)
        #[arg(long)]
        window: Option<String>,
        /// Filter: substring (case-insensitive)
        #[arg(long)]
        contains: Option<String>,
        /// Filter: regex (needs `regex` semantics)
        #[arg(long)]
        regex: Option<String>,
        /// Filter by role (e.g. "button")
        #[arg(long)]
        role: Option<String>,
        /// Filter by exact automation id (stable across relabels/locales)
        #[arg(long)]
        id: Option<String>,
    },
    /// Invoke (click) a control by name or id — no focus or coordinates needed
    Click {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
        /// Double-click instead of single click
        #[arg(long, default_value_t = false)]
        double: bool,
        /// Right-click instead of left click
        #[arg(long, default_value_t = false, conflicts_with = "double")]
        right: bool,
    },
    /// Toggle a checkbox/switch by name or id
    Toggle {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Set an edit/combo value by name or id
    SetValue {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] value: String,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Scroll into view + keyboard-focus a control, so `mk text` lands in it
    Focus {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Read a control's state (value / toggle / expand) as JSON
    GetValue {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Expand (or --collapse) a menu/combo/tree node
    Expand {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
        /// Collapse instead of expanding
        #[arg(long, default_value_t = false)]
        collapse: bool,
    },
    /// Wait until a control exists (and optionally is visible + enabled)
    Wait {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
        /// How long to wait, e.g. "10s" (default "10s")
        #[arg(long, default_value = "10s")]
        timeout: String,
        /// Poll interval, e.g. "400ms" (default "400ms")
        #[arg(long, default_value = "400ms")]
        interval: String,
        /// Also require enabled + on-screen (default: false, presence only)
        #[arg(long, default_value_t = false)]
        visible: bool,
    },
    /// Screenshot just one control (window capture cropped to its bounds)
    Shot {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
        /// Output image path (.png recommended)
        #[arg(long)] out: String,
        /// Padding px around the bounds (default: 8, tolerates shadows)
        #[arg(long, default_value_t = 8)]
        pad: u32,
        /// Zoom factor 1-8 (default: 1)
        #[arg(long, default_value_t = 1)]
        zoom: u32,
    },
    /// Search the whole desktop for a control (no --window needed)
    Find {
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Type text directly into a control (no focus juggling)
    Type {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
        /// Text to type
        #[arg(long)] text: String,
        /// Paste via clipboard (restored after) for long/Unicode text
        #[arg(long, default_value_t = false)]
        clipboard: bool,
    },
    /// Invoke the accessible context menu on a control
    Menu {
        #[arg(long)] window: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
    /// Drag one object onto another (sliders, drag-and-drop)
    Drag {
        #[arg(long)] window: Option<String>,
        /// Source control name
        #[arg(long)] from: Option<String>,
        /// Source control automation id
        #[arg(long)] from_id: Option<String>,
        /// Target control name
        #[arg(long)] to: Option<String>,
        /// Target control automation id
        #[arg(long)] to_id: Option<String>,
        #[arg(long)] contains: bool,
        #[arg(long)] regex: bool,
    },
}

#[derive(Subcommand)]
enum ClipboardAction {
    /// Print clipboard text to stdout (raw, no trailing newline added)
    Get,
}

#[derive(Subcommand)]
enum VisionAction {
    /// Crop (and optionally zoom) an image file: --region x,y,w,h [--zoom N]
    Crop {
        /// Input image path
        input: String,
        /// Output image path (.png → PNG, else JPEG)
        output: String,
        /// Crop region x,y,w,h (e.g. "100,200,800,600")
        #[arg(long)]
        region: String,
        /// Zoom factor 1-8 applied after crop (default: 1)
        #[arg(long, default_value_t = 1)]
        zoom: u32,
        /// JPEG quality 1-100 (default: 85, PNG ignores it)
        #[arg(long, default_value_t = 85)]
        quality: u8,
    },
    /// Print image dimensions as JSON
    Info {
        /// Image path
        input: String,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon (requires root)
    Start,
    /// Stop the running daemon
    Stop,
    /// Restart the running daemon (stop + start, requires root)
    Restart,
    /// Print (or --apply) a user systemd service for mk-daemon autostart
    Systemd {
        /// Actually write ~/.config/systemd/user/mk-daemon.service (no root needed)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },
    /// Print (or --apply) a udev rule so mk-daemon runs without root
    Install {
        /// Actually write /etc/udev/rules.d/99-mk-uinput.rules (needs root)
        #[arg(long, default_value_t = false)]
        apply: bool,
    },
    /// Check if daemon is running
    Status,
}

#[derive(Subcommand, Clone)]
enum ScheduledAction {
    /// Type a text message
    Text {
        /// The message to type
        message: String,
    },
    /// Press Enter
    Enter,
    /// Press a key combination
    Key {
        /// Key to press
        key: String,
    },
    /// Copy text to clipboard and paste
    Paste {
        /// Text to paste
        text: String,
        /// Shortcut key combination to trigger paste (default: ctrl+v)
        #[arg(short, long, default_value = "ctrl+v")]
        shortcut: String,
    },
    /// Wait for a duration
    Wait {
        /// Duration: "10s", "5m", "250ms"
        duration: String,
    },
    /// Move mouse cursor to coordinates
    Move {
        /// Destination X coordinate
        x: i32,
        /// Destination Y coordinate
        y: i32,
        /// Duration of progressive slide
        #[arg(short, long)]
        duration: Option<String>,
    },
    /// Click a mouse button at coordinates
    Click {
        /// X coordinate
        x: i32,
        /// Y coordinate
        y: i32,
        /// Button to click
        #[arg(short, long, default_value = "left")]
        button: String,
        /// Duration of progressive slide
        #[arg(short, long)]
        duration: Option<String>,
    },
    /// Drag the mouse
    Drag {
        /// Start X coordinate
        x1: i32,
        /// Start Y coordinate
        y1: i32,
        /// End X coordinate
        x2: i32,
        /// End Y coordinate
        y2: i32,
        /// Duration of slide
        #[arg(short, long)]
        duration: Option<String>,
    },
    /// Press and hold a mouse button
    MouseDown {
        /// Button to press (default: left)
        #[arg(default_value = "left")]
        button: String,
    },
    /// Release a mouse button
    MouseUp {
        /// Button to release (default: left)
        #[arg(default_value = "left")]
        button: String,
    },
    /// Scroll the mouse wheel
    Scroll {
        /// Number of scroll clicks
        #[arg(allow_hyphen_values = true)]
        clicks: i32,
        /// Scroll horizontally instead of vertically
        #[arg(long)]
        horizontal: bool,
    },
    /// Take a screenshot
    Screenshot {
        /// Path to save the PNG image
        path: String,
    },
}

impl ScheduledAction {
    fn to_command(&self) -> parser::Command {
        match self {
            ScheduledAction::Text { message } => parser::Command::Text(message.clone()),
            ScheduledAction::Enter => parser::Command::Enter,
            ScheduledAction::Key { key } => parser::Command::Key(key.clone()),
            ScheduledAction::Paste { text, shortcut } => parser::Command::Paste(text.clone(), shortcut.clone()),
            ScheduledAction::Wait { duration } => parser::Command::Wait(
                parser::parse_duration(duration).unwrap_or(Duration::from_secs(0)),
            ),
            ScheduledAction::Move { x, y, duration } => parser::Command::MouseMove(
                x.to_string(),
                y.to_string(),
                duration.clone().unwrap_or_else(|| "0s".to_string())
            ),
            ScheduledAction::Click { x, y, button, duration } => parser::Command::MouseClick(
                x.to_string(),
                y.to_string(),
                button.clone(),
                duration.clone().unwrap_or_else(|| "0s".to_string())
            ),
            ScheduledAction::Drag { x1, y1, x2, y2, duration } => parser::Command::MouseDrag(
                x1.to_string(),
                y1.to_string(),
                x2.to_string(),
                y2.to_string(),
                duration.clone().unwrap_or_else(|| "500ms".to_string())
            ),
            ScheduledAction::MouseDown { button } => parser::Command::MouseDown(button.clone()),
            ScheduledAction::MouseUp { button } => parser::Command::MouseUp(button.clone()),
            ScheduledAction::Scroll { clicks, horizontal } => parser::Command::MouseScroll(
                clicks.to_string(),
                horizontal.to_string()
            ),
            ScheduledAction::Screenshot { path } => parser::Command::Screenshot(path.clone(), false, 85),
        }
    }
}

fn main() -> Result<()> {
    // Windows gives the main thread a 1 MB stack; clap's generated parser for
    // mk's (large and growing) CLI overflows it in debug builds. Run everything
    // on an 8 MB thread instead — the same trick rustc itself uses. Release
    // builds never overflowed, but this keeps `cargo run` usable everywhere.
    match std::thread::Builder::new()
        .name("mk-main".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(real_main)
        .map_err(|e| anyhow::anyhow!("Failed to spawn main thread: {e}"))?
        .join()
    {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!("mk main thread panicked")),
    }
}

fn real_main() -> Result<()> {
    // Make the process per-monitor-DPI-aware so SetCursorPos/GetCursorPos (and xcap's
    // screenshot capture) operate in physical pixels, matching mk's coordinate contract.
    // Without this, an unaware process gets coordinates silently rescaled by Windows on
    // HiDPI displays (same class of bug fixed for Linux in commit 8680594).
    // NEEDS validation on real Windows hardware — no Windows machine available in this dev environment.
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Doctor => {
            return doctor::run();
        }
        Commands::Check { idle, active_window, focused } => {
            let min_idle = idle.as_deref().map(parser::parse_duration).transpose()?;
            let report = mk::check::run(&mk::check::CheckRequest { min_idle, active_window, focused });
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.ok {
                std::process::exit(1);
            }
            return Ok(());
        }
        Commands::Window { action } => {
            return handle_window(action);
        }
        Commands::Ui { action } => {
            return handle_ui(action);
        }
        Commands::Daemon { action } => {
            #[cfg(target_os = "linux")]
            {
                return match action {
                    DaemonAction::Start => daemon_start(),
                    DaemonAction::Stop => daemon_stop(),
                    DaemonAction::Restart => daemon_restart(),
                    DaemonAction::Install { apply } => daemon_install(apply),
                    DaemonAction::Systemd { apply } => daemon_systemd(apply),
                    DaemonAction::Status => daemon_status(),
                };
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = action;
                bail!("Daemon operations are only supported on Linux.");
            }
        }
        _ => {}
    }

    let real_backend = input::detect_backend()?;

    let backend: Box<dyn Backend> = if cli.dry_run {
        Box::new(DryRunBackend)
    } else {
        real_backend
    };

    // Scheduled commands (`in`/`at`) log by default — so there's always a
    // persistent record of whether and when a delayed action actually fired,
    // even if the user forgot `--log`. An explicit `--log` still wins.
    let default_log = matches!(cli.command, Commands::In { .. } | Commands::At { .. })
        .then(default_scheduled_log_path);
    let log_path = cli.log.clone().or(default_log);
    let mut logger = log_path.as_deref().map(Logger::new).transpose()?;

    // Record the "armed" moment for scheduled commands; the fire itself is
    // logged by the interpreter when the action runs after the wait.
    if let Some(l) = logger.as_mut() {
        match &cli.command {
            Commands::In { duration, .. } => {
                let _ = l.log("scheduled", &format!("in {duration}"), "armed");
            }
            Commands::At { time, .. } => {
                let _ = l.log("scheduled", &format!("at {time}"), "armed");
            }
            _ => {}
        }
    }

    let mut interp = Interpreter::new(backend.as_ref(), cli.dry_run, logger.as_mut());

    match cli.command {
        Commands::Text { message, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::Text(message)])?;
        }
        Commands::Enter { focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::Enter])?;
        }
        Commands::Key { key, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::Key(key)])?;
        }
        Commands::Wait { duration } => {
            let dur = parser::parse_duration(&duration)?;
            interp.run(&[parser::Command::Wait(dur)])?;
        }
        Commands::Paste { text, shortcut, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::Paste(text, shortcut)])?;
        }
        Commands::In { duration, action } => {
            let dur = parser::parse_duration(&duration)?;
            let cmd = action.to_command();
            if cli.dry_run {
                println!("[dry-run] waiting {dur:?} before executing 1 action(s)");
                interp.run(&[cmd])?;
            } else {
                println!("Waiting {dur:?}...");
                std::thread::sleep(dur);
                interp.run(&[cmd])?;
            }
        }
        Commands::At { time, action } => {
            let delay = scheduler::delay_until_time(&time)?;
            let cmd = action.to_command();
            if cli.dry_run {
                println!("[dry-run] at {time} (wait {delay:?}, 1 action(s))");
                interp.run(&[cmd])?;
            } else {
                println!("Waiting until {time} ({delay:?})...");
                std::thread::sleep(delay);
                interp.run(&[cmd])?;
            }
        }
        Commands::KeepAwake { interval, key } => {
            let dur = parser::parse_duration(&interval)?;
            scheduler::keep_awake_loop(dur, &key, backend.as_ref(), cli.dry_run)?;
        }
        Commands::Run { file } => {
            let content = std::fs::read_to_string(&file)
                .map_err(|e| anyhow::anyhow!("Failed to read script: {e}"))?;
            let base_dir = Path::new(&file)
                .parent()
                .unwrap_or(Path::new("."));

            let vars = HashMap::new();
            let commands = parser::parse_script_with_vars(&content, &vars, Some(base_dir))?;
            let commands = parser::resolve_includes(&commands, base_dir, &vars)?;

            interp.run(&commands)?;
        }
        Commands::PasteFile { path } => {
            interp.run(&[parser::Command::PasteFile(path)])?;
        }
        Commands::PasteDir { path } => {
            interp.run(&[parser::Command::PasteDir(path)])?;
        }
        Commands::Move { x, y, duration, focus } => {
            focus_first(focus.as_deref())?;
            let dur = duration.unwrap_or_else(|| "0s".to_string());
            interp.run(&[parser::Command::MouseMove(x.to_string(), y.to_string(), dur)])?;
        }
        Commands::Click { x, y, button, duration, focus } => {
            focus_first(focus.as_deref())?;
            let dur = duration.unwrap_or_else(|| "0s".to_string());
            interp.run(&[parser::Command::MouseClick(x.to_string(), y.to_string(), button, dur)])?;
        }
        Commands::Drag { x1, y1, x2, y2, duration, focus } => {
            focus_first(focus.as_deref())?;
            let dur = duration.unwrap_or_else(|| "500ms".to_string());
            interp.run(&[parser::Command::MouseDrag(x1.to_string(), y1.to_string(), x2.to_string(), y2.to_string(), dur)])?;
        }
        Commands::MouseDown { button, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::MouseDown(button)])?;
        }
        Commands::MouseUp { button, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::MouseUp(button)])?;
        }
        Commands::Scroll { clicks, horizontal, focus } => {
            focus_first(focus.as_deref())?;
            interp.run(&[parser::Command::MouseScroll(clicks.to_string(), horizontal.to_string())])?;
        }
        Commands::MousePos => {
            #[cfg(target_os = "windows")]
            {
                unsafe {
                    let mut pos = std::mem::zeroed();
                    if windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pos) != 0 {
                        println!("x={}, y={}", pos.x, pos.y);
                    } else {
                        eprintln!("Failed to get cursor position");
                    }
                }
            }
            #[cfg(target_os = "macos")]
            {
                match input::macos::cursor_position() {
                    Ok((x, y)) => println!("x={x}, y={y}"),
                    Err(e) => eprintln!("Failed to get cursor position: {e}"),
                }
            }
            #[cfg(target_os = "linux")]
            {
                // X11 only: Wayland exposes no protocol for global cursor position.
                match std::process::Command::new("xdotool")
                    .args(["getmouselocation", "--shell"])
                    .output()
                {
                    Ok(o) if o.status.success() => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        match mk::vision::parse_getmouselocation(&stdout) {
                            Some((x, y)) => println!("x={x}, y={y}"),
                            None => eprintln!("Failed to parse xdotool output (X11 only)"),
                        }
                    }
                    _ => eprintln!("mouse-pos on Linux needs xdotool on X11; on Wayland the cursor position is not queryable"),
                }
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
            {
                eprintln!("mouse-pos is only supported on Windows, macOS, and Linux/X11");
            }
        }
        Commands::Screenshot { path, window, monitor, raw, quality, cursor, crop, zoom, title } => {
            // Only read on the Windows cursor-capture branch below; unused elsewhere.
            let _format = if raw { mk::vision::ScreenshotFormat::Raw } else { mk::vision::ScreenshotFormat::Compressed };
            // --title resolves to a window id via an immediate (zero-timeout) lookup.
            // clap already rejects --window + --title together; the first arm is defensive.
            let window = match (window, title) {
                (Some(_), Some(_)) => bail!("--window and --title conflict"),
                (Some(id), None) => Some(id),
                (None, Some(t)) => {
                    let w = mk::windows::wait_for_window(&t, false, Duration::ZERO, Duration::from_millis(100))?;
                    Some(w.id)
                }
                (None, None) => None,
            };
            // New --crop/--zoom path: capture directly with post-process so the
            // parser::Command variants (no crop support, used by scripts) keep compiling.
            if crop.is_some() || zoom != 1 {
                if cli.dry_run {
                    println!("[dry-run] screenshot: save to {path} (crop={crop:?}, zoom={zoom})");
                } else if let Some(window_id) = window {
                    mk::vision::capture_window_with_options(&window_id, &path, _format, quality, &crop, zoom)?;
                } else {
                    let monitor_idx = monitor.unwrap_or(0);
                    if cursor {
                        #[cfg(target_os = "windows")]
                        {
                            mk::vision::capture_screen_with_cursor_options(&path, _format, quality, &crop, zoom)?;
                        }
                        #[cfg(any(target_os = "macos", target_os = "linux"))]
                        {
                            mk::vision::capture_screen_with_cursor_options(&path, _format, quality, &crop, zoom)?;
                        }
                        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                        {
                            mk::vision::capture_monitor_with_options(monitor_idx, &path, _format, quality, &crop, zoom)?;
                        }
                    } else {
                        mk::vision::capture_monitor_with_options(monitor_idx, &path, _format, quality, &crop, zoom)?;
                    }
                }
            } else if let Some(window_id) = window {
                interp.run(&[parser::Command::ScreenshotWindow(window_id, path, raw, quality)])?;
            } else {
                let monitor_idx = monitor.unwrap_or(0);
                if cursor {
                    #[cfg(target_os = "windows")]
                    {
                        mk::vision::capture_screen_with_cursor(&path, _format, quality)?;
                    }
                    #[cfg(any(target_os = "macos", target_os = "linux"))]
                    {
                        mk::vision::capture_screen_with_cursor(&path, _format, quality)?;
                    }
                    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                    {
                        interp.run(&[parser::Command::ScreenshotMonitor(monitor_idx, path, raw, quality)])?;
                    }
                } else {
                    interp.run(&[parser::Command::ScreenshotMonitor(monitor_idx, path, raw, quality)])?;
                }
            }
        }
        Commands::Clipboard { action } => match action {
            ClipboardAction::Get => {
                if cli.dry_run {
                    println!("[dry-run] read_clipboard");
                } else {
                    // Raw content, no added newline: the agent sees exactly what was copied.
                    print!("{}", mk::output::clipget::read_clipboard()?);
                }
            }
        },
        Commands::Monitors => {
            let list = mk::vision::list_monitors()?;
            let arr: Vec<serde_json::Value> = list
                .iter()
                .enumerate()
                .map(|(i, (name, x, y, w, h))| {
                    serde_json::json!({"index": i, "name": name, "x": x, "y": y, "width": w, "height": h})
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
        Commands::Vision { action } => match action {
            VisionAction::Crop { input, output, region, zoom, quality } => {
                let rect = mk::vision::parse_crop_rect(&region)?;
                let (w, h) = mk::vision::crop_image_file(&input, &output, rect, zoom, quality)?;
                println!("Cropped {input} -> {output} ({w}x{h})");
            }
            VisionAction::Info { input } => {
                let img = image::open(&input)
                    .map_err(|e| anyhow::anyhow!("Failed to open {input}: {e}"))?;
                println!("{}", serde_json::json!({"path": input, "width": img.width(), "height": img.height()}));
            }
        },
        Commands::Daemon { .. } | Commands::Doctor | Commands::Check { .. } | Commands::Window { .. } | Commands::Ui { .. } => unreachable!(),
    }

    Ok(())
}

/// Default log for scheduled (`in`/`at`) actions: `~/.local/share/mk/scheduled.log`
/// (XDG_DATA_HOME-aware), created on demand. Append-only, so the trail accrues.
fn default_scheduled_log_path() -> String {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            format!("{home}/.local/share")
        });
    let dir = format!("{base}/mk");
    let _ = std::fs::create_dir_all(&dir);
    format!("{dir}/scheduled.log")
}

/// Handle `mk window {list,active,focus,move,resize,minimize,maximize,restore,close}` — printing JSON for list/active so
/// the output is machine-consumable (e.g. by an agent picking a click target).
fn handle_window(action: WindowAction) -> Result<()> {
    use mk::windows;
    match action {
        WindowAction::List => {
            let list = windows::list_windows()?;
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        WindowAction::Active => {
            let active = windows::active_window()?;
            println!("{}", serde_json::to_string_pretty(&active)?);
        }
        WindowAction::Focus { id, title } => {
            let id = match (id, title) {
                (Some(_), Some(_)) => bail!("usa id o --title, no ambos"),
                (Some(id), None) => id,
                (None, Some(t)) => {
                    mk::windows::wait_for_window(&t, false, Duration::ZERO, Duration::from_millis(100))?.id
                }
                (None, None) => bail!("falta id o --title"),
            };
            windows::focus_window(&id)?;
            println!("Focused window {id}");
        }
        WindowAction::Move { id, x, y } => {
            windows::move_window(&id, x, y)?;
            println!("Moved window {id} to ({x}, {y})");
        }
        WindowAction::Resize { id, width, height } => {
            windows::resize_window(&id, width, height)?;
            println!("Resized window {id} to {width}x{height}");
        }
        WindowAction::Minimize { id } => {
            windows::minimize_window(&id)?;
            println!("Minimized window {id}");
        }
        WindowAction::Maximize { id } => {
            windows::maximize_window(&id)?;
            println!("Maximized window {id}");
        }
        WindowAction::Restore { id } => {
            windows::restore_window(&id)?;
            println!("Restored window {id}");
        }
        WindowAction::Close { id } => {
            windows::close_window(&id)?;
            println!("Closed window {id}");
        }
        WindowAction::Wait { title, exact, timeout, interval } => {
            let t = mk::parser::parse_duration(&timeout)?;
            let i = mk::parser::parse_duration(&interval)?;
            let w = windows::wait_for_window(&title, exact, t, i)?;
            println!("{}", serde_json::to_string_pretty(&w)?);
        }
        WindowAction::AltTab { count } => {
            // macOS switches apps with cmd+tab, everywhere else alt+tab.
            let combo = if cfg!(target_os = "macos") { "cmd+tab" } else { "alt+tab" };
            let backend = input::detect_backend()?;
            for n in 0..count {
                backend.press_key(combo)?;
                if n + 1 < count {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
            }
            println!("Sent {combo} {count} time(s)");
        }
    }
    Ok(())
}

fn match_mode(contains: bool, regex: bool) -> Result<mk::accessibility::MatchMode> {
    use mk::accessibility::MatchMode;
    match (contains, regex) {
        (false, false) => Ok(MatchMode::Exact),
        (true, false) => Ok(MatchMode::Contains),
        (false, true) => Ok(MatchMode::Regex),
        (true, true) => anyhow::bail!("usa --contains o --regex, no ambos"),
    }
}

/// Resolve `--name/--contains/--regex` vs `--id` into the (query, mode) the
/// backend expects. `--id` is exact-match on AutomationId and conflicts with
/// the name flags.
fn resolve_target(
    name: Option<String>,
    id: Option<String>,
    contains: bool,
    regex: bool,
) -> Result<(String, mk::accessibility::MatchMode)> {
    use mk::accessibility::MatchMode;
    match (name, id) {
        (Some(_), Some(_)) => anyhow::bail!("usa --name o --id, no ambos"),
        (None, Some(id)) => {
            if contains || regex {
                anyhow::bail!("--contains/--regex solo valen con --name")
            }
            Ok((id, MatchMode::AutomationId))
        }
        (Some(n), None) => Ok((n, match_mode(contains, regex)?)),
        (None, None) => anyhow::bail!("falta --name o --id"),
    }
}

/// Focus a window in-process before injecting input (avoids the focus-revert
/// that happens between separate CLI invocations — the #1 cause of actions
/// landing in the wrong window). Every input-injecting command takes --focus.
fn focus_first(focus: Option<&str>) -> Result<()> {
    if let Some(id) = focus {
        mk::windows::focus_window(id)?;
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Ok(())
}

/// Window id or, when omitted, the currently active window (so most `mk ui`
/// calls need no `window list` round-trip first).
fn resolve_window(window: Option<String>) -> Result<String> {
    use anyhow::Context;
    match window {
        Some(id) => Ok(id),
        None => Ok(mk::windows::active_window()
            .context("sin --window y sin ventana activa detectable (usa `mk window list`)")?
            .id),
    }
}

fn handle_ui(action: UiAction) -> Result<()> {
    use mk::accessibility::MatchMode;
    match action {
        UiAction::Tree { window, contains, regex, role, id } => {
            let window = resolve_window(window)?;
            let mut tree = mk::accessibility::ui_tree_for_window(&window)?;
            if let Some(sub) = contains {
                tree.retain(|e| mk::accessibility::match_name(&e.name, &sub, &MatchMode::Contains));
            }
            if let Some(re) = regex {
                tree.retain(|e| mk::accessibility::match_name(&e.name, &re, &MatchMode::Regex));
            }
            if let Some(r) = role {
                tree.retain(|e| e.role.eq_ignore_ascii_case(&r));
            }
            if let Some(want) = id {
                tree.retain(|e| e.automation_id == want);
            }
            println!("{}", serde_json::to_string_pretty(&tree)?);
        }
        UiAction::Click { window, name, id, contains, regex, double, right } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = if right {
                mk::accessibility::ui_right_click(&window, &query, &mode)?
            } else if double {
                mk::accessibility::ui_double_click(&window, &query, &mode)?
            } else {
                mk::accessibility::ui_click(&window, &query, &mode)?
            };
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Toggle { window, name, id, contains, regex } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_toggle(&window, &query, &mode)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::SetValue { window, name, id, value, contains, regex } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_set_value(&window, &query, &value, &mode)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Focus { window, name, id, contains, regex } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_focus(&window, &query, &mode)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::GetValue { window, name, id, contains, regex } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let st = mk::accessibility::ui_get_value(&window, &query, &mode)?;
            println!("{}", serde_json::to_string_pretty(&st)?);
        }
        UiAction::Expand { window, name, id, contains, regex, collapse } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let st = mk::accessibility::ui_expand(&window, &query, &mode, collapse)?;
            println!("{}", serde_json::to_string_pretty(&st)?);
        }
        UiAction::Wait { window, name, id, contains, regex, timeout, interval, visible } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let t = mk::parser::parse_duration(&timeout)?;
            let i = mk::parser::parse_duration(&interval)?;
            let el = mk::accessibility::ui_wait(&window, &query, &mode, t, i, visible)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Shot { window, name, id, contains, regex, out, pad, zoom } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_shot(&window, &query, &mode, &out, pad, zoom)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Find { name, id, contains, regex } => {
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let hits = mk::accessibility::ui_find(&query, &mode)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
        }
        UiAction::Type { window, name, id, contains, regex, text, clipboard } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_type(&window, &query, &mode, &text, clipboard)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Menu { window, name, id, contains, regex } => {
            let window = resolve_window(window)?;
            let (query, mode) = resolve_target(name, id, contains, regex)?;
            let el = mk::accessibility::ui_show_menu(&window, &query, &mode)?;
            println!("{}", serde_json::to_string_pretty(&el)?);
        }
        UiAction::Drag { window, from, from_id, to, to_id, contains, regex } => {
            let window = resolve_window(window)?;
            let (from_q, mode) = resolve_target(from, from_id, contains, regex)?;
            let (to_q, to_mode) = resolve_target(to, to_id, contains, regex)?;
            if mode != to_mode {
                bail!("--from/--to deben usar el mismo modo (ambos --name o ambos --id)");
            }
            let (from_el, to_el) = mk::accessibility::ui_drag(&window, &from_q, &to_q, &mode)?;
            println!("{}", serde_json::json!({"from": from_el, "to": to_el}));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn daemon_start() -> Result<()> {
    use std::process::Command;

    // Check if already running
    if input::daemon::daemon_is_running() {
        println!("mk-daemon is already running.");
        return Ok(());
    }

    println!("Starting mk-daemon (requires root)...");

    // Find the mk-daemon binary in the same directory as mk
    let mk_path = std::env::current_exe()?;
    let mk_dir = mk_path.parent().unwrap_or(Path::new("."));
    let daemon_path = mk_dir.join("mk-daemon");

    let status = Command::new("sudo")
        .arg(&daemon_path)
        .arg("--foreground")
        .status()
        .context("Failed to start mk-daemon. Is sudo available?")?;

    if !status.success() {
        bail!("mk-daemon exited with status: {status}");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn daemon_stop() -> Result<()> {
    use std::process::Command;

    let output = Command::new("pkill")
        .arg("-f")
        .arg("mk-daemon")
        .output()
        .context("Failed to run pkill")?;

    if output.status.success() {
        println!("mk-daemon stopped.");
    } else {
        println!("mk-daemon was not running.");
    }

    // Clean up socket
    let _ = std::fs::remove_file("/tmp/mk-daemon.sock");

    Ok(())
}

#[cfg(target_os = "linux")]
fn daemon_restart() -> Result<()> {
    daemon_stop()?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    daemon_start()
}

/// udev rule granting the `input` group rw access to /dev/uinput, so
/// mk-daemon runs without root after the user joins that group.
#[cfg(target_os = "linux")]
fn uinput_udev_rule() -> &'static str {
    "# mk uinput access — lets mk-daemon run without root.\nKERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\"\n"
}

#[cfg(target_os = "linux")]
fn daemon_install(apply: bool) -> Result<()> {
    use std::process::Command;

    const RULE_PATH: &str = "/etc/udev/rules.d/99-mk-uinput.rules";
    let rule = uinput_udev_rule();

    if !apply {
        println!("To run mk-daemon WITHOUT root, install this udev rule:\n");
        println!("{rule}");
        println!("Apply automatically with: sudo mk daemon install --apply");
        println!("Then:");
        println!("  1. sudo usermod -aG input $USER && re-login");
        println!("  2. sudo udevadm control --reload-rules && sudo udevadm trigger");
        println!("  3. start the daemon without sudo: mk-daemon &");
        return Ok(());
    }

    std::fs::write(RULE_PATH, rule)
        .context(format!("Failed to write {RULE_PATH} (run with sudo)"))?;
    let status = Command::new("udevadm")
        .args(["control", "--reload-rules"])
        .status()
        .context("Failed to run udevadm (is udev installed?)")?;
    if !status.success() {
        bail!("udevadm control --reload-rules failed");
    }
    println!("Installed {RULE_PATH}. Next:");
    println!("  1. sudo usermod -aG input $USER && re-login");
    println!("  2. sudo udevadm trigger");
    println!("  3. mk-daemon &");
    Ok(())
}

/// User systemd unit for mk-daemon autostart. Rootless by design: pair it
/// with `mk daemon install --apply` (udev rule) so /dev/uinput needs no root.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn systemd_unit(mk_daemon_path: &str) -> String {
    format!(
        "[Unit]\nDescription=mk virtual input daemon (uinput)\nAfter=graphical-session.target\n\n[Service]\nExecStart={mk_daemon_path} --foreground\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
    )
}

#[cfg(target_os = "linux")]
fn daemon_systemd(apply: bool) -> Result<()> {
    use std::process::Command;

    let mk_path = std::env::current_exe()?;
    let mk_dir = mk_path.parent().unwrap_or(Path::new("."));
    let daemon_path = mk_dir.join("mk-daemon");
    let unit = systemd_unit(&daemon_path.to_string_lossy());

    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME not set"))?;
    let unit_path = format!("{home}/.config/systemd/user/mk-daemon.service");

    if !apply {
        println!("User systemd unit for mk-daemon (pair with `mk daemon install --apply` for rootless uinput):\n");
        println!("{unit}");
        println!("Apply with: mk daemon systemd --apply");
        println!("Then: systemctl --user enable --now mk-daemon");
        return Ok(());
    }

    if let Some(parent) = Path::new(&unit_path).parent() {
        std::fs::create_dir_all(parent).context("Failed to create systemd user dir")?;
    }
    std::fs::write(&unit_path, &unit).context(format!("Failed to write {unit_path}"))?;
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("Failed to run systemctl (is systemd running?)")?;
    if !status.success() {
        bail!("systemctl --user daemon-reload failed");
    }
    println!("Wrote {unit_path}. Enable with: systemctl --user enable --now mk-daemon");
    Ok(())
}

#[cfg(target_os = "linux")]
fn daemon_status() -> Result<()> {
    if input::daemon::daemon_is_running() {
        println!("mk-daemon: running (socket /tmp/mk-daemon.sock)");
        match input::daemon::ping_daemon() {
            Ok(()) => println!("  Response: OK"),
            Err(e) => println!("  Ping failed: {e}"),
        }
        match input::daemon::daemon_version() {
            Ok(v) => println!("  Protocol version: {v}"),
            Err(e) => println!("  Protocol version: unknown ({e})"),
        }
    } else {
        println!("mk-daemon: not running");
        println!();
        println!("To start: sudo mk-daemon");
        println!("  or:     sudo mk daemon start");
    }
    Ok(())
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_scroll_negative_parses_without_double_dash() {
        let cli = Cli::try_parse_from(["mk", "scroll", "-6"]).expect("scroll -6 should parse");
        match cli.command {
            Commands::Scroll { clicks, horizontal, .. } => {
                assert_eq!(clicks, -6);
                assert!(!horizontal);
            }
            _ => panic!("expected Scroll"),
        }
    }

    #[test]
    fn test_ui_cli_parses() {
        let cli = Cli::try_parse_from(["mk", "ui", "tree", "--window", "123"]).expect("ui tree parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "click", "--window", "123", "--name", "Mezclador"]).expect("ui click parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "click", "10", "20", "--focus", "123"]).expect("click --focus parse");
        assert!(matches!(cli.command, Commands::Click { .. }));
    }

    #[test]
    fn test_alt_tab_parses_with_default_count() {
        let cli = Cli::try_parse_from(["mk", "window", "alt-tab"]).expect("alt-tab parse");
        match cli.command {
            Commands::Window { action } => match action {
                WindowAction::AltTab { count } => assert_eq!(count, 1),
                _ => panic!("expected AltTab"),
            },
            _ => panic!("expected Window"),
        }
        let cli = Cli::try_parse_from(["mk", "window", "alt-tab", "3"]).expect("alt-tab 3 parse");
        match cli.command {
            Commands::Window { action } => match action {
                WindowAction::AltTab { count } => assert_eq!(count, 3),
                _ => panic!("expected AltTab"),
            },
            _ => panic!("expected Window"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_udev_rule_content() {
        let rule = super::uinput_udev_rule();
        assert!(rule.contains("KERNEL==\"uinput\""));
        assert!(rule.contains("GROUP=\"input\""));
    }

    #[test]
    fn test_systemd_unit_content() {
        let unit = super::systemd_unit("/usr/local/bin/mk-daemon");
        assert!(unit.contains("ExecStart=/usr/local/bin/mk-daemon --foreground"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn test_vision_cli_parses() {
        let cli = Cli::try_parse_from(["mk", "vision", "crop", "a.png", "b.png", "--region", "10,20,300,200", "--zoom", "2"]).expect("vision crop parse");
        assert!(matches!(cli.command, Commands::Vision { .. }));
        let cli = Cli::try_parse_from(["mk", "vision", "info", "a.png"]).expect("vision info parse");
        assert!(matches!(cli.command, Commands::Vision { .. }));
        let cli = Cli::try_parse_from(["mk", "monitors"]).expect("monitors parse");
        assert!(matches!(cli.command, Commands::Monitors));
        let cli = Cli::try_parse_from(["mk", "clipboard", "get"]).expect("clipboard get parse");
        assert!(matches!(cli.command, Commands::Clipboard { .. }));
    }

    #[test]
    fn test_ui_object_commands_parse() {
        let cli = Cli::try_parse_from(["mk", "ui", "focus", "--window", "1", "--name", "X"]).expect("ui focus parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "get-value", "--window", "1", "--id", "btn1"]).expect("ui get-value parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "expand", "--window", "1", "--name", "M", "--collapse"]).expect("ui expand parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "wait", "--window", "1", "--name", "D"]).expect("ui wait parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "shot", "--window", "1", "--name", "D", "--out", "d.png"]).expect("ui shot parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "click", "--window", "1", "--name", "B", "--double"]).expect("ui click double parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        // --double + --right conflict
        assert!(Cli::try_parse_from(["mk", "ui", "click", "--window", "1", "--name", "B", "--double", "--right"]).is_err());
    }

    #[test]
    fn test_ui_find_type_and_optional_window_parse() {
        // --window omitted: resolves to the active window at runtime.
        let cli = Cli::try_parse_from(["mk", "ui", "click", "--name", "B"]).expect("ui click without window");
        match cli.command {
            Commands::Ui { action } => match action {
                UiAction::Click { window, .. } => assert!(window.is_none()),
                _ => panic!("expected Click"),
            },
            _ => panic!("expected Ui"),
        }
        let cli = Cli::try_parse_from(["mk", "window", "focus", "--title", "Code"]).expect("focus --title parse");
        match cli.command {
            Commands::Window { action } => match action {
                WindowAction::Focus { id, title } => {
                    assert!(id.is_none());
                    assert_eq!(title.as_deref(), Some("Code"));
                }
                _ => panic!("expected Focus"),
            },
            _ => panic!("expected Window"),
        }
    }

    #[test]
    fn test_resolve_target_modes() {
        use mk::accessibility::MatchMode;
        let (q, m) = super::resolve_target(Some("B".into()), None, false, false).unwrap();
        assert_eq!((q.as_str(), m), ("B", MatchMode::Exact));
        let (q, m) = super::resolve_target(None, Some("btn1".into()), false, false).unwrap();
        assert_eq!((q.as_str(), m), ("btn1", MatchMode::AutomationId));
        assert!(super::resolve_target(Some("A".into()), Some("b".into()), false, false).is_err());
        assert!(super::resolve_target(None, None, false, false).is_err());
        assert!(super::resolve_target(None, Some("b".into()), true, false).is_err());
        assert!(super::resolve_target(Some("A".into()), None, true, true).is_err());
    }

    #[test]
    fn test_ui_menu_drag_parse() {
        let cli = Cli::try_parse_from(["mk", "ui", "menu", "--name", "Item"]).expect("ui menu parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "drag", "--from", "A", "--to", "B"]).expect("ui drag parse");
        match cli.command {
            Commands::Ui { action } => match action {
                UiAction::Drag { from, to, from_id, to_id, .. } => {
                    assert_eq!(from.as_deref(), Some("A"));
                    assert_eq!(to.as_deref(), Some("B"));
                    assert!(from_id.is_none() && to_id.is_none());
                }
                _ => panic!("expected Drag"),
            },
            _ => panic!("expected Ui"),
        }
        let cli = Cli::try_parse_from(["mk", "ui", "find", "--name", "Save"]).expect("ui find parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
        let cli = Cli::try_parse_from(["mk", "ui", "type", "--id", "box1", "--text", "hola", "--clipboard"]).expect("ui type parse");
        assert!(matches!(cli.command, Commands::Ui { .. }));
    }
}
