# Computer-use with mk — practical skill notes

> Extracted from a live autonomous session on GNOME/Wayland (2026-07-07),
> per the user's goal: *"lo mejor para sacar las skills es que un agente itere
> sobre cómo se usa"*. These are the concrete, learned-the-hard-way rules for
> driving a desktop through `mk` as an AI agent.

## Prefer the accessibility tree

Act by name first; it is cheaper and more exact than reading images:

```
mk ui find --name "..."  →  mk ui click / focus / get-value  →  mk ui get-value or mk ui tree
```

- Confirm results by reading the tree, not by taking a screenshot.
- Use screenshots when the tree cannot help: apps that draw everything themselves,
  apps not exposing a tree, or checking how something looks.
- Keystrokes go to the active window. Before typing unattended, gate it with
  `mk check --idle ... --active-window ...`, and prefer `mk ui type`, which refuses
  to type into a window that is not active.
- `mk doctor` lists the windows exposed. A missing Electron app must be restarted
  with `ACCESSIBILITY_ENABLED=1 <app> --force-renderer-accessibility`.

## The core loop when you need coordinates (non-negotiable discipline)

```
move mouse  →  screenshot  →  READ & validate cursor/target  →  click/adjust  →  screenshot
```

One interaction per step. Never chain blind clicks. Every mouse action is
followed by a screenshot you actually read before the next action. This is
slow on purpose — a wrong click can close the window that hosts your own
session.

## Coordinates — the HiDPI scaling trap (read this)

`mk screenshot` writes a PNG in **PHYSICAL** pixels (here 2880×1920). Whether
`mk click x,y` (using those same physical pixels) lands correctly depends on
the display scale:

- **Unscaled (1×) display:** clicks are 1:1 with the screenshot. Straight­forward.
- **Fractionally-scaled display (this machine: 1.667× on GNOME/Wayland):**
  there was a **bug** — clicks landed off by the scale factor toward the
  bottom-right (a click aimed at the "File" menu opened "Edit"). Root cause:
  xcap reports the **logical** size for `Monitor::width()/height()`
  (1728×1152) but captures screenshots in **physical** pixels (2880×1920);
  `scale_coords` mapped the virtual absolute pointer across the logical size,
  so every coordinate was scaled wrong.
  - **Fixed** (2026-07-07, `src/input/daemon.rs`): `get_screen_resolution`
    now returns `logical × scale_factor` = physical, matching the screenshot.
    After the fix, raw screenshot coordinates land correctly (verified:
    `mk click 89 75` opens "File" as intended). **Rebuild + reinstall the
    daemon/client for the fix to take effect.**
  - **If you're on an unpatched `mk` under fractional scaling**, the manual
    workaround is: `mk click (px / scale)`, i.e. divide screenshot
    coordinates by the display scale factor (get it from
    `~/.config/monitors.xml` `<scale>` on GNOME, or `xcap` `scale_factor`).
    For 1.667×: `mk_x = screenshot_x / 1.667`.

- Screenshots do **not** capture the cursor, so `mk move x y` can't be
  visually confirmed by position. In practice the ideal
  "move→screenshot→validate→click" degrades to **"identify target from the
  current screenshot → click → screenshot the RESULT → adjust"** — validation
  happens *after* the click. So always aim at a target that gives clear
  feedback when hit (a menu opens, a field's focus ring appears), and near the
  origin first when calibrating (scale error is smallest there).
- Take the first screenshot before anything and re-check the reported image
  dimensions **and the display scale** each session.

## Gotchas that cost real attempts

1. **Scheduled `mk` typing lands in whatever window has focus.** A
   `mk in/at ... paste "..." && mk enter` types into the *currently focused*
   window when it fires — not into any particular app. If you switch focus
   away and it fires, the message is corrupted / lands in the wrong app.
   Rule: if a scheduled message is pending (or mid-typing), **do not change
   window focus**; let it complete first. (Observed: a note was being typed
   character-by-character into the agent's own input while navigating — any
   window switch would have split it.)

2. **Prefer `mk key alt+tab` over overview-clicking to switch windows.**
   `mk key alt+tab` reliably raises the previous window (verified: brought the
   Claude PWA to the foreground first try). Clicking a thumbnail in the
   `mk key super` overview is far less reliable — partly the overview can
   close before the click registers, but the bigger culprit was the HiDPI
   coordinate bug above (clicks missed the thumbnail and hit the app behind).
   With the scale fix, overview-clicking is more usable, but alt+tab is still
   the simpler, keyboard-only path for "go to that other window".

3. **`mk window list` is blind on GNOME/Wayland.** It enumerates via XCB and
   sees only XWayland surfaces — the real apps (browsers, IDEs, PWAs) are
   Wayland-native and invisible. Do **not** rely on it to find a window's
   coordinates on Wayland. (It *is* reliable on X11, Windows, macOS.) See
   `docs/window-control.md`.

4b. **Typing/pasting into a web-app text field needs an accurate click first.**
   Early attempts to `mk paste` into the Claude PWA's input failed — because
   the coordinate bug meant the "focus the input" click never actually hit it
   (no cursor in the screenshot to reveal the miss). The lesson: you cannot
   confirm a text field is focused visually before typing; land the focus
   click accurately (post-fix), verify via a screenshot that shows the caret/
   focus ring, and only then paste. Targeting the field by name with `mk ui`
   avoids all of this.

4. **Never click these:** the GNOME-overview per-window close **✗** (top-right
   of the hovered window), and an app's own window-control buttons. In
   overview the ✗ appears over the *hovered/primary* window — verify which
   window it's on before clicking anywhere near a top-right corner.

## Working recipes (GNOME/Wayland)

- **See the whole desktop / find a window visually:** `mk key super` → wait
  ~700ms → `mk screenshot`. Read the thumbnails to locate the target app.
- **Switch focus (when overview-click is flaky):** `mk key alt+tab` (hold/
  repeat to cycle). More reliable than clicking overview thumbnails for the
  *previous* window; for a specific window, overview + careful click.
- **Type into a specific input:** first click the input to focus it
  (screenshot to confirm the caret/highlight), then `mk text "..."` or
  `mk paste "..."`. Confirm with a screenshot before pressing `mk enter`.
- **Schedule a wake-up into a specific session:** leave that session's input
  focused, then `mk at HH:MM paste "..." && mk enter` (or `mk in <dur> ...`).
  Because focus decides the destination, don't touch the GUI afterwards.

## What mk can and can't do here (summary)

| Want to…                    | GNOME/Wayland | How |
|-----------------------------|:-------------:|-----|
| Screenshot the screen       | ✅            | `mk screenshot` (portal/PipeWire) |
| Move/click/scroll/type      | ✅            | `mk move/click/scroll/text/paste` (daemon) |
| Cycle window focus          | ✅ (blind)    | `mk key alt+tab` / `super` + click |
| List windows and the active one | ✅ if exposed | `mk window list`, through AT-SPI |
| Focus a specific window by id | ❌          | not a Wayland operation; use overview/alt-tab |
| Read UI element tree (a11y) | ✅            | `mk ui tree`, through AT-SPI |
| Press a control by name     | ✅            | `mk ui click`, window need not be active |
| Know if it is safe to act   | ✅            | `mk check` |

On Windows/macOS most of the ❌ rows become ✅ — see `docs/window-control.md`.
