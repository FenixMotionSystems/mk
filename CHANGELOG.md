# Changelog

## [Unreleased]

### Added
- `mk ui` en Linux vía AT-SPI, sin Python ni GTK. `mk window list` enseña las ventanas nativas de Wayland que AT-SPI expone.
- `mk check`: comprueba inactividad, ventana activa y control con foco; sale con 1 si algo falla o no se sabe.
- `mk doctor` informa de la accesibilidad.
- `mk text` respeta la distribución del teclado en Linux (antes, con teclado español, `/` salía como `-`). El demonio aprende `altgr` y `102nd`, así que hay que reiniciarlo.
- Con ydotool, `mk key` reconoce las letras: un `_ => None` adelantado las dejaba inalcanzables.
- `mk mouse-pos` en macOS (nativo) y Linux/X11 (vía `xdotool getmouselocation`); en Wayland error honesto.
- `mk screenshot --cursor` en macOS y Linux (antes solo Windows; en Linux se arregla además el comando roto `xdotool getcursor`).
- Protocolo daemon `VERSION` + `mk daemon restart` / versión en `mk daemon status`; `mk doctor` reporta daemon, versión de protocolo y acceso a `/dev/uinput`.
- Daemon `KEY:` con tecla desconocida responde `ERR:unknown key` en vez de OK silencioso.

### Fixed
- `capture_screen_with_cursor` en Linux usaba `xdotool getcursor` (comando inexistente); ahora `getmouselocation --shell`.

### Added
- `mk window alt-tab [n]` (Wayland-safe switcher; `cmd+tab` en macOS).
- `mk daemon install [--apply]`: regla udev para correr mk-daemon sin root.
- `mk doctor` detecta IPC de compositores opt-in (`hyprctl`, `swaymsg`).
- `mk window focus <id>` como comando script (`focus`), además de `--focus` en move/click/scroll.
- `mk clipboard get` (lee el portapapeles en las 3 plataformas: pbpaste/macOS, wl-paste/xclip/xsel/Linux, CF_UNICODETEXT/Windows).
- `mk monitors` (lista monitores como JSON con geometría).
- `mk screenshot --title <substring>` (captura la ventana coincidente inmediatamente, sin `mk window wait` previo).
- `mk ui` por objetos: `--id` (AutomationId estable) en todos los subcomandos; `mk ui focus` (ScrollIntoView + SetFocus para `mk text`); `mk ui get-value` (lee valor/toggle/expand); `mk ui expand [--collapse]`; `mk ui wait --timeout --visible`; `mk ui shot --out` (captura solo el control); `click --double/--right`; fallback a click nativo UIA cuando no hay InvokePattern.
- `mk ui find` (búsqueda en todo el escritorio con contexto de ventana), `mk ui type --text [--clipboard]` (escribe directo en el control), `--window` opcional en `mk ui` (= ventana activa), `mk window focus --title`.
- `mk ui menu` (menú contextual accesible), `mk ui drag --from/--to` (objeto a objeto), `mk paste` por portapapeles nativo en Windows (rápido para textos largos), acordes en `mk key` (`ctrl+s`, `alt+tab`...), `--focus` en todos los comandos de input (text/enter/key/paste/drag/mouse-down/up).
- `mk window focus/move/resize` vía Hyprland (`hyprctl dispatch`) y Sway (`swaymsg`) cuando aplica; best-effort en mosaicos, garantizado en flotantes.
- `mk key` honesto en ydotool/wtype: tabla evdev completa (dígitos, F1-F12, puntuación) y errores en vez de tragar teclas (`ctrl+1` ya no pulsa solo ctrl, `a+b` ya no pierde la `a`).
- Backend macOS para `mk ui` (System Events vía `osascript`, sin deps nuevas): `tree/click/double/right/menu/toggle/set-value/type/focus/wait/shot` con escalado Retina; `get-value/expand/find/drag` devuelven error honesto.
- Helpers `accessibility` compartidos (`shot_element`, `wait_for_element`) para no duplicar entre backends Windows/macOS.
- `mk vision crop <in> <out> --region x,y,w,h [--zoom N]` y `mk vision info <img>` (post-procesar sin recapturar).
- `mk window list` en Linux prueba Hyprland (`hyprctl clients -j`) y Sway (`swaymsg -t get_tree`) antes de xcb, con `pid` y foco reales.
- `mk daemon systemd [--apply]`: servicio de usuario systemd para autostart rootless (con la regla udev).

## [0.7.0] - Unreleased

### Added
- `mk ui tree/click/toggle/set-value` (Windows UIA, exact-match por defecto, `--contains/--regex` opt-in, flags `is_enabled/is_offscreen`).
- `mk window wait --title` (poll con `--timeout/--interval`).
- `pid` en `mk window list` (Windows).
- `mk screenshot --crop x,y,w,h --zoom N`.
- `--focus <id>` en `move/click/scroll` + `mk scroll -6` sin `--`.

## [0.6.0] - 2026-06-22

### Added
- **Reorganización Modular de Librería:**
  - Estructuración de las APIs en cinco módulos semánticos principales: `input` (emulación de entrada), `output` (portapapeles y pegado), `vision` (capturas de pantalla y sub-regiones), `windows` (enumeración y enfoque de ventanas) y `accessibility` (árbol de accesibilidad UI).
- **Captura por Región de Pantalla:**
  - Nueva función `capture_region` para recortar y guardar sub-regiones específicas de la pantalla, optimizando el ancho de banda y el consumo de tokens visuales.
- **Gestión Multiplataforma de Ventanas:**
  - Enumeración y enfoque de ventanas activas (`list_windows`, `active_window`, `focus_window`) integrando `xcap` y llamadas nativas o scripts específicos (Win32 en Windows, AppleScript en macOS y `xdotool` en Linux).
- **Stubs de Accesibilidad:**
  - Estructuras y contratos base (`UiElement` y stubs `get_ui_tree`, `find_button`, `find_input`) para futura interacción semántica en Computer Use.

## [0.5.0] - 2026-06-21

### Added
- **Soporte Nativo Multiplataforma (Windows y macOS):**
  - Implementación del backend nativo de Windows (`WindowsBackend`) utilizando la API Win32 `SendInput` y `SetCursorPos` de forma directa y ultra ligera.
  - Implementación del backend nativo de macOS (`MacosBackend`) utilizando las APIs del framework Core Graphics (`CGEvent`) para inyectar eventos de entrada nativos de sistema de manera segura.
  - Detección compile-time y conditional compilation para empaquetar únicamente las dependencias de cada plataforma (`windows-sys` en Windows, `core-graphics` y `foreign-types` en macOS).
  - Portabilidad completa para los comandos de escritura (`text`), teclado (`key`), ratón (`move`, `click`, `drag`, `mouse-down`, `mouse-up`, `scroll`) y capturas de pantalla (`screenshot`) en los tres sistemas operativos principales.
- **Soporte de duraciones compuestas:**
  - El analizador de duraciones ahora acepta formatos compuestos y espaciados como `"1h 53m"`, `"1h53m"` o `"2h 30m 10s"`, sumando automáticamente sus componentes para mayor comodidad.

## [0.4.0] - 2026-06-21

### Added
- **Soporte de Ratón Virtual (uinput):**
  - Implementación de ejes absolutos (rango `0..32767`) y botones (`left`, `right`, `middle`) para simular interacciones táctiles y de puntero absolutas.
  - Soporte de scroll vertical y horizontal en el daemon (`SCROLL`).
  - Movimiento progresivo/suave de cursor interpolado en el tiempo (`MOVE_SMOOTH`).
- **Nuevos comandos de CLI y Scripting:**
  - `move <x> <y> [-d <duración>]`: Desplazar el cursor.
  - `click <x> <y> [-b <botón>] [-d <duración>]`: Clic de ratón.
  - `drag <x1> <y1> <x2> <y2> [-d <duración>]`: Arrastrar.
  - `mouse-down [<botón>]` y `mouse-up [<botón>]`: Presionar/soltar botones de ratón individualmente.
  - `scroll <clicks> [-h]`: Scroll de rueda de ratón (vertical/horizontal).
  - `screenshot <ruta>`: Captura de pantalla multiplataforma nativa.
- **Resolución dinámica:**
  - Detección automática de resolución de pantalla activa mediante `xcap` y escalado dinámico a las coordenadas absolutas de uinput.
- **Variables dinámicas en scripting:**
  - Los comandos del ratón guardan sus campos como textos para permitir la expansión tardía de variables en runtime (con `set` y `exec`).
