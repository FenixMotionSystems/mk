# Window control & full desktop automation — design & honest reality check

> Status: research + first increment, 2026-07-07 (night session, autonomous).
> Author: Claude (blazer agent), acting on the user's directive:
> *"que mk pueda controlar el pc casi tirando de win32, winapis, mac apis y
> linux apis ajenas a gnome, wayland, x11, hyprland o kde"* — i.e. control
> the machine via low-level OS APIs, independent of the desktop environment.

## TL;DR (read this first)

The "DE-independent low-level window control" vision is **fully achievable on
Windows and macOS**, achievable-but-legacy on **X11**, and **architecturally
impossible on Wayland** — including this machine (GNOME/Mutter on Wayland).
This is not a missing feature we can add; it's Wayland's security model by
design. The compositor is the *sole* authority over window enumeration,
focus, geometry and stacking, and it deliberately exposes **no** generic,
DE-independent protocol or syscall for a client to do these to *other*
windows.

Therefore mk's realistic path to "automate ~100%" is **not** a universal
window-control API. It is:

1. **Input simulation** (mk's existing strength: `ydotool`/`wtype`/`libei`,
   and win32/CoreGraphics natively) — Alt+Tab, Super/overview + click,
   coordinate clicks, typing. This *is* DE-independent at the input layer and
   already works on Wayland.
2. **Native window control on Windows & macOS** (win32 / AXUIElement) — here
   the full vision is real.
3. **Optional compositor backends on Linux** (hyprctl, swaymsg, KWin
   scripting) for users who run those — explicitly DE-*specific*, opt-in.
4. **Honest capability detection** so callers (e.g. an AI agent) know which
   operations are available on the current session instead of silently
   failing.

## Empirical evidence gathered on this machine

Session: GNOME on **Wayland** (`XDG_SESSION_TYPE=wayland`, `WAYLAND_DISPLAY=wayland-0`).

- `mk screenshot` **works** — xcap uses the Wayland screencast portal /
  PipeWire path. Full-screen capture at native 2880×1920.
- `mk window list` (new, this session; xcap `Window::all()` under the hood)
  returns **only one window**: a residual X11 surface titled `"Widgets"`.
  The real application windows — Antigravity/VS Code, the Claude PWA, the
  terminal — are **Wayland-native and invisible to it**. `mk window active`
  reports *no* focused window at all.
- Root cause: xcap enumerates Linux windows via **XCB (X11)**. On a Wayland
  session that only reaches XWayland clients; Mutter's native Wayland windows
  are not X11 objects, so `_NET_ACTIVE_WINDOW`/`_NET_CLIENT_LIST` (what XCB
  reads) simply don't describe them.

Concretely: on this machine an agent **cannot** discover or focus the Claude
PWA window programmatically through any window API. The reliable way to reach
it is input simulation (`mk key super` → overview → click a thumbnail), which
is exactly what the live experiment that motivated this doc had to fall back
to — and even that is coordinate-fragile because there is no window geometry
to aim at.

## Why Wayland forbids this (not a bug, a design axiom)

Under X11, any client can walk the whole window tree, read other windows'
titles/contents, warp the pointer, and raise/lower/move anyone — the root of
X11's well-known input-security problems. Wayland was designed to close
exactly that: a client can only see and touch **its own** surfaces. Global
operations (list all windows, focus another app, move a foreign window,
global hotkeys, read the screen) are delegated to the **compositor** and
surfaced, if at all, only through *opt-in, mediated* channels:

- `xdg-desktop-portal` — `ScreenCast` (capture, with a user consent dialog),
  `RemoteDesktop` (input injection, with consent). **No** window-management
  portal exists.
- `libei` / `ei` — the emerging standard for *emulated input* on Wayland
  (the modern replacement for the ydotool/uinput approach). Input only.
- Compositor-private IPC — Hyprland (`hyprctl`), Sway (`swaymsg`, i3-IPC),
  KDE/KWin (scripting over D-Bus). Powerful, but **DE-specific** and absent
  on GNOME.
- GNOME/Mutter — deliberately exposes **no** stable public window-management
  API. (`org.gnome.Shell.Eval` is disabled by default; third-party
  extensions like "Window Calls" can expose window lists over D-Bus, but
  that is an installed extension, not a low-level OS API.)

So "linux apis ajenas a gnome, wayland, x11..." that still control windows do
**not exist**: below the compositor there is only the kernel (uinput/evdev for
*input*, DRM/KMS for *display*), and neither has any concept of a "window".
Input injection is reachable that low (uinput) — window management is not.

## Per-platform feasibility matrix

| Capability                    | Windows (win32) | macOS (AX/CG) | Linux X11 (xcb) | Linux Wayland |
|-------------------------------|:---------------:|:-------------:|:---------------:|:-------------:|
| Enumerate windows + geometry  | ✅ EnumWindows  | ✅ CGWindowList | ✅ xcb/EWMH    | ❌ (compositor only) |
| Read focused window           | ✅ GetForegroundWindow | ✅ AX | ✅ _NET_ACTIVE_WINDOW | ❌ |
| Focus/raise a window by id    | ✅ SetForegroundWindow | ✅ AX/AppleScript | ✅ EWMH msg | ❌ (Alt+Tab/overview only) |
| Move/resize a foreign window  | ✅ SetWindowPos | ✅ AX         | ✅ ConfigureWindow | ❌ / compositor IPC |
| Inject keyboard/mouse         | ✅ SendInput    | ✅ CGEvent    | ✅ XTest        | ✅ uinput/libei (mk-daemon) |
| Screen capture                | ✅ BitBlt/DXGI  | ✅ CG         | ✅ xcb          | ✅ portal/PipeWire (xcap) |
| UI-element tree (a11y)        | ✅ UIA          | ✅ AXUIElement | ~ AT-SPI       | ~ AT-SPI (app-dependent) |

Legend: ✅ feasible with a native/low-level API · ~ partial/opt-in · ❌ not
possible generically.

## Recommended architecture for mk

Keep the current cross-platform **input** core (mk-daemon + ydotool/wtype;
win32 `SendInput`; macOS `CGEvent`) — it is the DE-independent foundation and
already works. Layer window control as a **capability-detected backend**, not
a promise of universality:

```
mk window ...
  ├─ native backend   (Windows: win32 · macOS: AX/CGWindowList)   → full
  ├─ x11 backend      (xcb/EWMH, incl. XWayland-only visibility)  → full-ish
  ├─ compositor backend (opt-in: hyprctl / swaymsg / kwin)        → full, DE-specific
  └─ wayland fallback (no window API) → expose input-simulation helpers:
        mk window focus-interactive  → `key super` + guided click
        mk window alt-tab [n]        → cycle focus by simulated Alt+Tab
     and report is_active/geometry as "unknown" honestly.
```

`mk doctor` should grow a "Window control" section reporting the detected
backend and exactly which of the matrix rows are available on this session,
so an agent can plan instead of trial-and-error.

## Phased plan

- **Phase 0 (done this session):** `mk window {list,active,focus}` wired to
  the existing (previously unexposed) `windows` module; dropped the broken
  `xdotool`-based active detection in favour of xcap's native `is_focused()`;
  Wayland `focus` now returns an honest, actionable error instead of shelling
  out to an X11-only tool that isn't installed. Added `WindowInfo::center()`
  for "click the window body" targeting. JSON output for agent consumption.
- **Phase 1:** `mk doctor` window-control capability report + capability enum
  returned from the library, so callers detect before they act.
- **Phase 2:** Solidify the two platforms where the full vision is real —
  Windows (`EnumWindows`/`SetWindowPos`/`GetWindowRect`) and macOS
  (`CGWindowListCopyWindowInfo` + `AXUIElement` for focus/move). This is where
  "control the PC via win32/mac APIs" is genuinely delivered.
- **Phase 3:** Wayland input-simulation helpers (`alt-tab`, `focus-interactive`,
  overview-driven selection) — the pragmatic substitute for focus-by-id.
- **Phase 4 (opt-in, PARTIALLY DONE):** compositor backends behind runtime
  detection. `mk window list` tries Hyprland (`hyprctl clients -j`) and Sway
  (`swaymsg -t get_tree`) before xcb, and `focus`/`move`/`resize` dispatch
  through them (`focuswindow`/`movewindowpixel`/`resizewindowpixel`,
  `[con_id] focus`/`move position`/`resize set`). KWin still pending.
  Caveats: pixel move/resize is best-effort on tiled layouts (guaranteed on
  floating); minimize/maximize/close stay X11-only for now.
- **Phase 5 (IN PROGRESS):** accessibility tree. Windows UIA done
  (`mk ui`, 13 subcommands). macOS partial: System Events dump
  (`src/accessibility/macos_ax.rs`, no new deps) wires tree/click/focus/
  wait/shot; value/expand/global-find bail honestly. Linux AT-SPI done over
  blocking zbus (`src/accessibility/linux_atspi.rs`) and validated on
  GNOME/Wayland against VS Code; only double-click, drag and shot bail.
  **Field evidence (2026-07-20, Windows, see `docs/windows-computer-use-notes.md`):**
  raw coordinate control (`mk move`/`click`/`scroll`) worked correctly once
  window offset + focus were handled right, but named-element clicks via
  Windows UI Automation (done outside mk, via `AutomationElement.FindFirst`
  + `InvokePattern.Invoke()`) succeeded on the first try every time, needed
  no coordinates/focus/scrolling-into-view, and doubled as a tree-search
  debugging tool that caught a real UI bug coordinate-poking never would
  have found. On Windows specifically, this makes Phase 5 higher-leverage
  than further coordinate-precision work — the `uiautomation` crate (or raw
  `windows-rs` UIA bindings) maps almost directly onto the existing
  `UiElement{role,name,x,y,width,height}` stub shape.
  **Update 2026-09-10 (done en Windows 0.7.0 vía `uiautomation` 0.25.1;
  Linux/mac siguen stub):** `mk ui tree/click/toggle/set-value` implementado
  (`--name` exact-match por defecto, `--contains/--regex` opt-in,
  `is_enabled/is_offscreen` en `UiElement`); ver `docs/windows-computer-use-notes.md`.

## Honest bottom line for the roadmap

- The full low-level vision **ships on Windows and macOS** — pursue it there.
- On **Linux Wayland (this machine's reality)** the vision is blocked at the
  protocol level; mk's realistic superpower there is **robust input
  simulation + screenshots** (both already working), optionally augmented by
  compositor-specific IPC for non-GNOME setups.
- Do **not** invest in trying to make xcb/X11 window control work on GNOME
  Wayland — the empirical test above shows it sees essentially nothing.
