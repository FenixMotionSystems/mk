# mk

CLI multiplataforma para automatizar escritura, pulsaciones de teclas, simulaciones de ratón y capturas de pantalla en Linux, Windows y macOS.

## Sistemas Operativos y Backends soportados

- **Linux**: Detecta automáticamente si estás en Wayland o X11 para seleccionar el backend adecuado (`wtype`, `xdotool`, `ydotool` o el daemon virtual uinput `mk-daemon`).
- **Windows**: Automatización nativa utilizando la API Win32 (`SendInput` y `SetCursorPos`). No requiere dependencias adicionales ni daemons externos.
- **macOS**: Automatización nativa utilizando el framework Core Graphics (`CGEvent`). No requiere dependencias adicionales ni daemons externos (se requieren permisos de Accesibilidad en Preferencias del Sistema).

| Backend / SO | Linux (Wayland) | Linux (X11) | Windows | macOS |
|--------------|-----------------|-------------|---------|-------|
| `wtype`      | ✅              | ✅          | ❌      | ❌    |
| `xdotool`    | ❌              | ✅          | ❌      | ❌    |
| `ydotool`    | ✅              | ❌          | ❌      | ❌    |
| `mk-daemon`  | ✅ (uinput)     | ✅ (uinput) | ❌      | ❌    |
| `Win32 API`  | ❌              | ❌          | ✅      | ❌    |
| `CoreGraph`  | ❌              | ❌          | ❌      | ✅    |

## Instalación

```bash
cargo install --path .
```

Los backends deben estar instalados en el sistema:

```bash
# Arch Linux
sudo pacman -S wtype xdotool

# Ubuntu/Debian
sudo apt install wtype xdotool
```

## Uso

### Escribir texto

```bash
mk text "Hola mundo"
```

### Presionar Enter

```bash
mk enter
```

### Combinaciones de teclas

```bash
mk key "ctrl+s"
mk key "ctrl+c"
mk key "alt+tab"
mk key "ctrl+alt+Delete"
```

### Esperar un tiempo

```bash
mk wait "5s"
mk wait "10m"
mk wait "2h"
```

### Pegar texto (portapapeles + Ctrl+V)

```bash
mk paste "texto copiado"
```

En Wayland usa `wl-copy`, en X11 usa `xclip` o `xsel`. Si no hay herramienta de portapapeles, usa `type_text` como fallback.

### Simulación de ratón

- **En Linux**: Se requiere que `mk-daemon` esté activo para abrir el dispositivo táctil absoluto en `/dev/uinput` y poder interactuar de forma segura en Wayland y X11.
- **En Windows y macOS**: Las acciones de ratón funcionan directamente de forma nativa e instantánea sin necesidad de ejecutar ningún daemon ni requerir permisos adicionales más allá de Accesibilidad (en macOS).

```bash
# Mover cursor (de forma progresiva en 500ms o instantánea si se omite la duración)
mk move 500 500 --duration "500ms"

# Hacer clic (botón izquierdo por defecto, o custom con -b)
mk click 500 500 --button right --duration "200ms"

# Arrastrar (presiona botón izquierdo, se desplaza y luego lo suelta)
mk drag 100 100 800 800 --duration "1s"

# Presionar y soltar botones de forma persistente
mk mouse-down left
mk mouse-up left

# Hacer scroll (clicks positivos hacia arriba/derecha, negativos hacia abajo/izquierda)
mk scroll 3
mk scroll --horizontal -- -2
```

Las coordenadas de pantalla `(X, Y)` en píxeles se detectan de forma automática respecto a tu monitor principal y se escalan transparentemente a la tableta absoluta.

### Posición del cursor y daemon (Linux)

```bash
# Ver posición actual del cursor (Windows/macOS nativo; Linux solo X11 con xdotool)
mk mouse-pos

# Ver monitores (JSON con geometría, para targeting multi-monitor)
mk monitors

# Recortar/zoom una captura ya guardada + ver dimensiones
mk vision crop shot.png detalle.png --region 100,200,800,600 --zoom 2
mk vision info shot.png

# Gestionar mk-daemon (Linux, requiere root para /dev/uinput)
sudo mk-daemon            # arrancar en foreground
sudo mk daemon start      # arrancar
sudo mk daemon stop       # parar
sudo mk daemon restart    # reiniciar
mk daemon status          # estado + versión de protocolo
```

### Captura de pantalla (Screenshots)

Toma una captura de pantalla del monitor primario y la guarda en la ruta indicada de forma nativa:

```bash
mk screenshot ruta/de/mi_imagen.png
```

Para leer un detalle pequeño, recorta y amplía tras la captura:

```bash
mk screenshot detalle.png -w 329272 --raw --crop 100,200,800,600 --zoom 2
```

### Control semántico de UI — `mk ui` (Windows, vía UI Automation)

En Windows, `mk ui` actúa sobre controles por **nombre o automation-id** (sin
coordenadas ni foco previo): usa `Invoke`/`Toggle`/`ValuePattern` directamente
sobre el control. `--name` es exact-match (tras `trim()`, case-insensitive)
por defecto; `--contains` / `--regex` son opt-in. `--id` busca por
AutomationId exacto (estable ante re-etiquetas e idiomas) y no se combina
con `--name`.

| Comando                                   | Descripción                              |
|-------------------------------------------|------------------------------------------|
| `mk ui tree --window <id>`                | Lista elementos UI de la ventana (JSON)  |
| `mk ui find --name "..."`                 | Busca en todo el escritorio (devuelve ventana + elemento) |
| `mk ui click --window <id> --name "..."`  | Invoca (clic semántico) un control       |
| `mk ui type --name "..." --text "..."`    | Escribe directo en el control (`--clipboard` para texto largo) |
| `mk ui focus --window <id> --name "..."`  | Lleva a vista + foco de teclado (para `mk text`) |
| `mk ui get-value --window <id> --name "..."` | Lee valor/toggle/expand del control   |
| `mk ui toggle --window <id> --name "..."` | Conmuta un checkbox/switch               |
| `mk ui set-value --window <id> --name "..." --value "..."` | Escribe valor en un edit/combo |
| `mk ui expand --window <id> --name "..."` | Expande menú/combo/nodo (`--collapse` para colapsar) |
| `mk ui wait --window <id> --name "..." --timeout 10s` | Espera a que exista el control (`--visible` exige visible+habilitado) |
| `mk ui shot --window <id> --name "..." --out d.png` | Captura solo ese control |
| `mk ui menu --window <id> --name "..."` | Abre el menú contextual accesible del control |
| `mk ui drag --window <id> --from A --to B` | Arrastra un objeto sobre otro |

```bash
mk window list
mk ui find --name "Mezclador"          # sin --window: busca en todo el escritorio
mk ui tree                             # sin --window: usa la ventana activa
mk ui click --id "btnMezclar"          # AutomationId: estable ante idiomas
mk ui type --name "Preset" --text "Init" --clipboard
mk window focus --title "Mezclador"    # foco por título, sin id previo
mk paste "texto largo" --focus 2230160  # --focus en TODOS los comandos de input (text/enter/key/paste/click/move/...)
```

> Regla de oro para agentes: **todo input lleva `--focus <id>` en la misma
> invocación** (o usa `mk ui`, que no necesita foco). Sin eso, el foco puede
> revertir entre llamadas y la acción aterriza en otra ventana.

### `mk ui` en Linux (vía AT-SPI, sin Python ni GTK)

Mismas órdenes y mismo JSON que en Windows. Pulsar un control funciona aunque
su ventana no sea la activa. `mk window list` también enseña las ventanas nativas
de Wayland que AT-SPI expone, con su id `atspi:...`.

- **Apps Electron/Chromium (VS Code y derivados):** solo aparecen si arrancan con
  `ACCESSIBILITY_ENABLED=1 <app> --force-renderer-accessibility`. `mk doctor`
  lista lo que se ve. VS Code puede pasar entonces a su modo para lectores de
  pantalla.
- **`mk ui type` se niega a escribir** si la ventana del control no es la activa:
  las teclas siempre van a la ventana activa, y en Wayland no se puede activar una
  ventana cualquiera desde fuera.
- **`mk ui set-value`** depende de la app: si no lo admite, usa `mk ui type --clipboard`.
- **No disponibles:** `double-click`, `drag` y `shot`. En Wayland, `x`/`y` de un
  elemento son relativos a su ventana: actúa por nombre, no por esos números.

### `mk check`: ¿es seguro actuar?

Imprime JSON y sale con código 1 si una condición falla o el sistema no puede
responderla, así que se encadena con `&&`. Una respuesta desconocida nunca cuenta
como «adelante».

| Condición | Significa |
|-----------|-----------|
| `--idle 30m` | Sin tocar teclado ni ratón ese tiempo (por ahora, solo GNOME) |
| `--active-window "Code"` | El título de la ventana activa contiene ese texto |
| `--focused "Message input"` | El control con el foco contiene ese nombre (por ahora, solo Linux) |

```bash
# Compactar Claude Code de noche, solo si nadie está usando el PC
mk wait 50m && mk check --idle 30m --active-window "Visual Studio Code" \
  && mk ui type --name "Message input" --text "/compact" --clipboard \
  && mk ui click --name "Send message"
```

### Teclado en `mk text` (Linux)

`mk text` respeta la distribución activa: con teclado español, `/` se envía como
`shift+7`. Un carácter que necesita tecla muerta (`á`) da error sin escribir nada:
usa `mk paste`. Tras actualizar, reinicia el demonio (`mk daemon restart`).

### `mk ui` en macOS (vía System Events, sin dependencias nuevas)

Mismos subcomandos; el árbol AX se vuelca con un `osascript` y los clics van
por CGEvent al centro del control. Coordenadas AX (puntos lógicos) se escalan
a píxeles físicos (Retina). Funciona: `tree/click/double/right/menu/toggle/
set-value/type/focus/wait/shot`. Aún no: `get-value/expand/find/drag`
(error honesto). Receta mac: `mk ui click --name X` y luego `mk text`.

 Relacionado (también 0.7.0): `mk window wait --title "<app>" --timeout 10s`
 (espera a que aparezca una ventana), campo `pid` en `mk window list`
 (Windows), y `--focus <id>` en `move`/`click`/`scroll` para enfocar la
 ventana en el mismo proceso antes de actuar (p. ej. `mk click 500 500
 --focus 329272`). `mk scroll -6` ya funciona sin el separador `--`.

### Ejecutar un script

```bash
mk run mi_script.mk
```

### Dry-run (sin ejecutar)

```bash
mk --dry-run run mi_script.mk
mk --dry-run text "prueba"
```

### Logging

```bash
mk --log acciones.log run mi_script.mk
```

Cada acción registra timestamp, nombre y resultado.

### Diagnóstico del sistema

```bash
mk doctor
```

Muestra sesión, backends disponibles, herramientas de portapapeles y recomendaciones.

## Formato de scripts

Los scripts `.mk` son archivos de texto con un comando por línea:

```bash
# Comentarios con #
text "Hola mundo"
wait "1s"
key "ctrl+a"
text "texto seleccionado"
enter
wait "500ms"
key "ctrl+s"
```

### Comandos disponibles

| Comando      | Descripción                         | Ejemplo                          |
|--------------|-------------------------------------|----------------------------------|
| `text`       | Escribir texto                      | `text "Hola"`                    |
| `enter`      | Presionar Enter                     | `enter`                          |
| `key`        | Presionar tecla                     | `key "ctrl+s"`                   |
| `wait`       | Esperar                             | `wait "5s"`                      |
| `paste`      | Copiar y pegar via portapapeles     | `paste "texto"`                  |
| `paste-file` | Pegar archivo formateado (Markdown) | `paste-file "src/main.rs"`       |
| `paste-dir`  | Pegar dir de código recursivo       | `paste-dir "src"`                |
| `exec`       | Ejecutar comando y guardar en var   | `exec var "cargo test"`          |
| `set`        | Definir variable                    | `set nombre "Claude"`            |
| `repeat`     | Repetir bloque N veces              | `repeat 3 { ... }`               |
| `include`    | Incluir otro archivo                | `include "common.mk"`            |
| `move`       | Mover cursor (opcional duración)    | `move 500 500 "500ms"`           |
| `click`      | Clic en coordenadas                 | `click 500 500 "left" "200ms"`   |
| `drag`       | Arrastre de cursor                  | `drag 10 10 100 100 "1s"`        |
| `mouse-down` | Presionar botón                     | `mouse-down "left"`              |
| `mouse-up`   | Soltar botón                        | `mouse-up "left"`                |
| `scroll`     | Desplazar rueda del ratón           | `scroll -3 "false"`              |
| `screenshot` | Captura de pantalla                 | `screenshot "foto.png"`          |

### Duraciones soportadas

Se admiten duraciones simples y compuestas (separadas opcionalmente por espacios):
- `Nms` — milisegundos (ej: `250ms`)
- `Ns` — segundos (ej: `5s`)
- `Nm` — minutos (ej: `10m`)
- `Nh` — horas (ej: `2h`)

Ejemplos de duraciones compuestas válidas:
- `"1h 53m"`
- `"1h53m"`
- `"2h 30m 10s 500ms"`

### Variables

```bash
set name "Claude"
set lang "es"
text "Hola ${name}"
paste "Mi nombre es ${name}"
```

### Bloques repeat

```bash
repeat 3 {
    text "hola"
    enter
    wait "1s"
}
```

Soporta anidación:

```bash
repeat 2 {
    repeat 3 {
        text "x"
    }
}
```

### Include

`common.mk`:
```bash
text "común"
enter
```

`main.mk`:
```bash
text "antes"
include "common.mk"
text "después"
```

Los paths son relativos al archivo que contiene el `include`.

## Ejemplo completo

```bash
# Abrir una terminal, escribir un comando y ejecutarlo
mk run abrir_terminal.mk
```

`abrir_terminal.mk`:
```bash
key "ctrl+alt+t"
wait "1s"
text "ls -la"
enter
wait "500ms"
text "exit"
enter
```

Script con variables y repeat:
```bash
set user "admin"
repeat 3 {
    text "login: ${user}"
    enter
    wait "500ms"
}
key "ctrl+d"
```

## Documentación y Manuales

Para un tutorial detallado con explicaciones paso a paso de cada función, la configuración segura del daemon y ejemplos prácticos para interactuar con LLMs (como Claude o Gemini) en español, consulta el archivo [TUTORIAL.md](file:///home/javiju/proyectos/mk/TUTORIAL.md).

