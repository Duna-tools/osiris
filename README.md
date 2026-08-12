<div align="center">

<a id="english"></a>

# Osiris

### MCP server for Godot projects

English | [Español](#espanol)

![rust](https://img.shields.io/badge/language-rust-orange)
![edition](https://img.shields.io/badge/edition-2024-blue)
![os](https://img.shields.io/badge/os-linux%2C%20win%2C%20mac-lightgrey)
![godot](https://img.shields.io/badge/godot-4.x-green)
![license](https://img.shields.io/badge/license-Apache_2.0-blue)

</div>

**Osiris is a local MCP (Model Context Protocol) server that inspects, modifies and controls Godot projects. A Rust core talks to a GDScript bridge running inside the Godot editor over TCP loopback, giving MCP clients (Claude Desktop, Cursor, VS Code, and any stdio MCP client) typed access to scenes, properties, execution, runtime observation and safety policy.**

## Key Features

### Scene Inspection
> Read-only access to the editor state.

- **Active scene**: path, root name and node count.
- **Node tree**: paginated traversal with depth limit and node cap.
- **Properties and resources**: read/write any node property (or resource) with automatic Variant to JSON coercion (Vector, Color, Transform); list referenced resources.
- **Class reflection**: `godot.describe` introspects the ClassDB (inheritance, properties, methods, signals, constants).
- **Project files & settings**: enumerate source files and read/write `project.godot` settings.

### Safe Modification
> Every mutation is reversible, permissioned and undo-aware.

- **Create, remove, move and instance nodes**: operations go through `EditorUndoRedoManager`; Ctrl+Z undoes them.
- **`scene.apply`**: apply a list of scene operations (create/remove/move/set/instance) in one call, each with Undo/Redo.
- **Scripts**: validate and write GDScript to disk; attach scripts to nodes via resource assignment.
- **Signals**: list, connect and disconnect with deferred/one-shot flags.
- **Plans**: `plan.execute` runs a sequence of tools with `$id.field` references between steps.
- **Capability profiles**: `read` (default), `edit` (mutations), `admin` (project execution); denied by default.
- **Idempotency**: repeated reads are cached (FIFO, 128 entries); mutations are never auto-retried.
- **Audit log**: every tool call is logged (actor, tool, sanitized params, policy, effect, duration) to stderr and optionally to a JSONL file (`--audit-file`).

### Project Control
> Run, observe and assert.

- **Run and stop** the project via the editor's own debugger (`play_custom_scene`), capturing output and errors from the running game.
- **Runtime observation**: remote scene tree (`godot.runtime.tree`), 59 performance monitors (`godot.runtime.metrics`), object inspection (`debug.inspect`) and screenshots (`runtime.capture`) via the debugger protocol.
- **Run history**: every execution is persisted to `user://osiris_run_history.jsonl` and queryable via `osiris.run.history`.
- **Assertions** on observable node state.

### Protocol Security
- **stdio** for MCP: zero network surface.
- **TCP loopback + token** for IPC (0600 file).
- Path traversal blocked (`..`, `//`, absolute paths rejected).
- Stable error codes, no internal detail leakage; logs on stderr only.

## Quick Start

### Step 1: Build

```bash
cargo build --release
```

### Step 2: Enable the bridge in your Godot project

```bash
cd your-godot-project
mkdir -p addons
ln -s /path/to/osiris/godot/osiris-bridge addons/osiris-bridge
```

Enable the plugin in Project Settings, Plugins. Start the editor. The bridge writes its token to `res://.godot/osiris_ipc_token` (stable per project).

### Step 3: Run the server

The easiest way with a Godot project open is the wrapper:

```bash
cd your-godot-project
/path/to/osiris/scripts/osiris-mcp.sh   # detects the project, uses admin profile
```

Or manually:

```bash
godot --editor --path your-godot-project &
TOKEN=$(cat your-godot-project/.godot/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

Default profile is `read`. Use `--capability-profile edit` (mutations) or `admin` (project execution).

## Configuration

Precedence: **CLI args > env vars > TOML file > defaults**.

```
osiris-server [options]

  --config <path>           TOML config file (or OSIRIS_CONFIG)
  --log-level <level>       Log level [default: info]
  --ipc-addr <addr>         Godot bridge address (or OSIRIS_IPC_ADDR)
  --ipc-token-file <path>   Bridge token file (or OSIRIS_IPC_TOKEN_FILE)
  --ipc-timeout-ms <n>      IPC operation timeout (or OSIRIS_IPC_TIMEOUT_MS) [default: 3000]
  --capability-profile <p>  read | edit | admin [default: read]
  --audit-file <path>       JSONL audit log (or OSIRIS_AUDIT_FILE)
  -h, --help
```

## Tools

| Tool | Profile | Description |
|---|---|---|
| `osiris_diagnose` | read | Server name, version, protocol versions |
| `osiris.state` | read | Current state machine (editor, game, debugger session, input) |
| `osiris.capabilities` | read | Catalog of capabilities, tools, Godot and protocol |
| `osiris.run.history` | read | Persisted run history (scene, duration, output, errors) |
| `godot_diagnose` | read | Godot version + project name |
| `godot.describe` | read | ClassDB reflection (inheritance, props, methods, signals) |
| `godot.scene.active` | read | Active scene path, root name, node count |
| `godot.scene.nodes` | read | Node tree (depth, pagination) |
| `godot.scene.properties` | read | Read a node property |
| `godot.scene.resources` | read | List resources referenced by a node |
| `godot.editor.errors` | read | Runtime errors of the running game (via debugger) |
| `godot.scene.create_node` | edit | Create a node (undo/redo) |
| `godot.scene.duplicate` | edit | Duplicate a node (deep copy, undo/redo) |
| `godot.scene.instance` | edit | Instance a packed scene (res://*.tscn) as child |
| `godot.scene.remove_node` | edit | Remove a node (undo/redo) |
| `godot.scene.set_property` | edit | Change a property (undo/redo) |
| `godot.scene.save` | edit | Save active scene to disk |
| `godot.scene.open` | edit | Open a scene (res://*.tscn) in the editor |
| `godot.scene.create` | edit | Create a new scene (res://*.tscn) and open it |
| `godot.scene.select` | edit | Select nodes in the editor tree (visual feedback) |
| `godot.scene.screenshot` | read | Capture the editor viewport (2D/3D) to a PNG |
| `godot.project.autoload` | edit | Register an autoload singleton in project.godot |
| `godot.inputmap.add` | edit | Register an InputMap action and persist it |
| `godot.scenes.open` | read | List the scenes currently open in the editor |
| `godot.export` | edit | Export the project to an export preset (release/debug) |
| `scene.apply` | edit | Apply a list of scene operations in one call |
| `object.call` | edit | Call a method on a node or resource with args, supports $res references |
| `object.property.get` | read | Read a property of a node or resource |
| `object.property.set` | edit | Write a property of a node (undo/redo) or resource |
| `godot.script.validate` | read | Validate GDScript syntax |
| `godot.script.write` | edit | Validate and write a GDScript file |
| `godot.signal.list` | read | List signals of a node |
| `godot.signal.connect` | edit | Connect a signal to a method |
| `godot.signal.disconnect` | edit | Disconnect a signal |
| `godot.signal.emit` | edit | Emit a signal with optional arguments |
| `godot.asset.list` | read | List assets with import status |
| `godot.asset.reimport` | edit | Reimport an asset |
| `godot.resource.assign` | edit | Load a resource and assign it to a property |
| `godot.resource.create` | edit | Create a new resource (Animation, Theme, SpriteFrames, ...) |
| `godot.project.files` | read | List project source files |
| `godot.project.settings.get` | read | Read project settings |
| `godot.project.settings.set` | edit | Write and persist project settings |
| `godot.run` | admin | Run the project (with debugger) |
| `godot.input.send` | admin | Inject an input (key or action) into the running game |
| `godot.stop` | admin | Stop the project |
| `godot.run.status` | read | Check if the project is running |
| `godot.run.output` | read | Output of the running game |
| `godot.runtime.tree` | read | Remote scene tree of the running game |
| `godot.runtime.metrics` | read | Performance monitors of the running game |
| `debug.inspect` | read | Inspect a runtime object via the debugger |
| `runtime.capture` | read | Screenshot of the running game |
| `plan.validate` | read | Validate a plan's structure |
| `plan.execute` | edit | Execute plan steps with `$id.field` references |
| `skill.list` | read | List available skills |
| `skill.get` | read | Skill details (tools used, knowledge) |

## Per-client MCP setup

The token lives at `your-project/.godot/osiris_ipc_token` (stable) and at `~/.local/share/godot/app_userdata/<Project>/osiris_ipc_token`. Use an absolute path if the binary is not on `PATH`; use `--log-level debug` for diagnostics (logs go to stderr).

**Automatic installer** — register Osiris in every installed LLM CLI (opencode, codex, agy, Claude) in one shot:

```bash
scripts/osiris-install.sh          # install into all detected CLIs (idempotent)
scripts/osiris-install.sh --remove # remove from all detected CLIs
```

While a session is running, the CLI shows Osiris in its status bar (e.g. opencode shows `⊙ N MCP` counting the connected Osiris server).

**Generic stdio client** (any MCP client that launches a local command) — use the wrapper to auto-detect the Godot project and the admin profile:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/path/to/osiris/scripts/osiris-mcp.sh"
    }
  }
}
```

**Claude Desktop** — `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/path/to/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876", "--capability-profile", "edit"]
    }
  }
}
```

**Cursor** — project MCP settings:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/path/to/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876"]
    }
  }
}
```

**VS Code (mcp extension)** — `.mcp.json`:

```json
{
  "servers": {
    "osiris": {
      "command": "/path/to/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876"]
    }
  }
}
```

## Testing

```bash
cargo test --workspace                 # unit + integration + property tests
cargo bench -p osiris-ipc-client       # codec ~1µs; real bridge roundtrip ~100ms
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                            # 0 known vulnerabilities
```

The suite covers lifecycle, handshake, stdout purity, size limits, capability authorization, idempotency, IPC reconnect, health, run output, plans, skills, runtime observation, and property-based fuzzing of the parsers.

## Security

- MCP transport is **stdio**; no network surface for client communication.
- IPC transport is **TCP loopback** + random token file (0600).
- Paths are validated: `..`, `//` and absolute paths are rejected.
- Error messages use stable category codes; never expose internal details.
- Logs write to **stderr**; stdout is reserved for JSON-RPC.
- Capabilities are **deny-by-default**; mutations require an explicit profile upgrade.
- Full dependency audit: 0 known vulnerabilities, 0 `unsafe` blocks.

## References

- [DECISIONS.md](DECISIONS.md): architecture decisions and technical debt.
- [COMPATIBILITY.md](COMPATIBILITY.md): supported version matrix.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to contribute.

## License

Released under the [Apache 2.0 license](LICENSE).

Copyright (c) 2026 Duna-tools.

---

<a id="espanol"></a>
<div align="center">

# Osiris

### Servidor MCP para proyectos Godot

Español | [English](#english)

![rust](https://img.shields.io/badge/language-rust-orange)
![edition](https://img.shields.io/badge/edition-2024-blue)
![os](https://img.shields.io/badge/os-linux%2C%20win%2C%20mac-lightgrey)
![godot](https://img.shields.io/badge/godot-4.x-green)
![license](https://img.shields.io/badge/license-Apache_2.0-blue)

</div>

**Osiris es un servidor MCP (Model Context Protocol) local que inspecciona, modifica y controla proyectos Godot. Un núcleo Rust se comunica con un puente GDScript dentro del editor Godot vía TCP loopback, dando a los clientes MCP (Claude Desktop, Cursor, VS Code y cualquier cliente MCP stdio) acceso tipado a escenas, propiedades, ejecución, observación de runtime y política de seguridad.**

## Características

### Inspección de escenas
> Acceso de solo lectura al estado del editor.

- **Escena activa**: ruta, nombre raíz y número de nodos.
- **Árbol de nodos**: recorrido paginado con límite de profundidad y tope de nodos.
- **Propiedades y recursos**: lectura/escritura de cualquier propiedad de un nodo (o recurso) con coerción automática Variant a JSON (Vector, Color, Transform) y listado de recursos referenciados.
- **Reflexión de clases**: `godot.describe` inspecciona el ClassDB (herencia, propiedades, métodos, señales, constantes).
- **Archivos y settings del proyecto**: enumerar archivos fuente y leer/escribir settings de `project.godot`.

### Modificación segura
> Toda mutación es reversible, con permisos y con undo.

- **Crear, eliminar, mover e instanciar nodos**: las operaciones pasan por `EditorUndoRedoManager`; Ctrl+Z las deshace.
- **`scene.apply`**: aplica una lista de operaciones de escena (crear/eliminar/mover/poner/instanciar) en una sola llamada, cada una con Undo/Redo.
- **Scripts**: validar y escribir GDScript a disco; asignar scripts a nodos vía asignación de recursos.
- **Señales**: listar, conectar y desconectar con flags deferred/one-shot.
- **Planes**: `plan.execute` ejecuta una secuencia de tools con referencias `$id.campo` entre pasos.
- **Perfiles de capacidades**: `read` (por defecto), `edit` (mutaciones), `admin` (ejecución); negado por defecto.
- **Idempotencia**: las lecturas repetidas se cachean (FIFO, 128 entradas); las mutaciones nunca se reintentan solas.
- **Registro de auditoría**: cada llamada a tool se registra (actor, tool, params sanitizados, política, efecto, duración) en stderr y opcionalmente en JSONL (`--audit-file`).

### Control de proyecto
> Ejecutar, observar y verificar.

- **Ejecutar y detener** el proyecto vía el debugger propio del editor (`play_custom_scene`), capturando salida y errores del juego en ejecución.
- **Observación de runtime**: árbol remoto de escena (`godot.runtime.tree`), 59 monitores de rendimiento (`godot.runtime.metrics`), inspección de objetos (`debug.inspect`) y capturas de pantalla (`runtime.capture`) vía el protocolo de debugger.
- **Historial de ejecuciones**: cada ejecución se persiste en `user://osiris_run_history.jsonl` y es consultable con `osiris.run.history`.
- **Aserciones** sobre el estado observable de nodos.

### Seguridad del protocolo
- **stdio** para MCP: cero superficie de red.
- **TCP loopback + token** para IPC (archivo 0600).
- Path traversal bloqueado (`..`, `//`, rutas absolutas rechazadas).
- Códigos de error estables, sin fuga de detalle interno; logs solo en stderr.

## Inicio rápido

### Paso 1: Compilar

```bash
cargo build --release
```

### Paso 2: Activar el puente en tu proyecto Godot

```bash
cd tu-proyecto-godot
mkdir -p addons
ln -s /ruta/a/osiris/godot/osiris-bridge addons/osiris-bridge
```

Activa el plugin en Ajustes del Proyecto, Plugins. Inicia el editor. El puente escribe su token en `res://.godot/osiris_ipc_token` (estable por proyecto).

### Paso 3: Ejecutar el servidor

La forma más fácil con un proyecto Godot abierto es el wrapper:

```bash
cd tu-proyecto-godot
/ruta/a/osiris/scripts/osiris-mcp.sh   # detecta el proyecto, usa perfil admin
```

O manualmente:

```bash
godot --editor --path tu-proyecto-godot &
TOKEN=$(cat tu-proyecto-godot/.godot/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

El perfil por defecto es `read`. Usa `--capability-profile edit` (mutaciones) o `admin` (ejecución).

## Configuración

Precedencia: **args CLI > variables de entorno > archivo TOML > valores por defecto**.

```
osiris-server [opciones]

  --config <ruta>           Archivo TOML de configuración (o OSIRIS_CONFIG)
  --log-level <nivel>       Nivel de log [defecto: info]
  --ipc-addr <addr>         Dirección del puente Godot (o OSIRIS_IPC_ADDR)
  --ipc-token-file <ruta>   Archivo con el token (o OSIRIS_IPC_TOKEN_FILE)
  --ipc-timeout-ms <n>      Timeout de operaciones IPC (o OSIRIS_IPC_TIMEOUT_MS) [defecto: 3000]
  --capability-profile <p>  read | edit | admin [defecto: read]
  --audit-file <ruta>       Log de auditoría JSONL (o OSIRIS_AUDIT_FILE)
  -h, --help
```

## Herramientas

| Herramienta | Perfil | Descripción |
|---|---|---|
| `osiris_diagnose` | read | Nombre, versión y versiones de protocolo |
| `osiris.state` | read | Estado actual (editor, juego, sesión debugger, input) |
| `osiris.capabilities` | read | Catálogo de capacidades, tools, Godot y protocolo |
| `osiris.run.history` | read | Historial de ejecuciones persistido (escena, duración, salida, errores) |
| `godot_diagnose` | read | Versión de Godot + nombre del proyecto |
| `godot.describe` | read | Reflexión del ClassDB (herencia, props, métodos, señales) |
| `godot.scene.active` | read | Escena activa: ruta, nombre raíz, nodos |
| `godot.scene.nodes` | read | Árbol de nodos (profundidad, paginación) |
| `godot.scene.properties` | read | Leer una propiedad de un nodo |
| `godot.scene.resources` | read | Recursos referenciados por un nodo |
| `godot.editor.errors` | read | Errores de runtime del juego (vía debugger) |
| `godot.scene.create_node` | edit | Crear un nodo (undo/redo) |
| `godot.scene.remove_node` | edit | Eliminar un nodo (undo/redo) |
| `godot.scene.set_property` | edit | Cambiar una propiedad (undo/redo) |
| `godot.scene.save` | edit | Guardar escena activa |
| `godot.scene.open` | edit | Abrir una escena (res://*.tscn) en el editor |
| `godot.scene.create` | edit | Crear una escena nueva (res://*.tscn) y abrirla |
| `godot.scene.select` | edit | Seleccionar nodos en el árbol del editor (feedback visual) |
| `godot.scene.screenshot` | read | Capturar el viewport del editor (2D/3D) a PNG |
| `godot.project.autoload` | edit | Registrar un autoload singleton en project.godot |
| `godot.inputmap.add` | edit | Registrar una acción de InputMap y persistirla |
| `godot.scenes.open` | read | Listar las escenas abiertas en el editor |
| `godot.export` | edit | Exportar el proyecto a un preset (release/debug) |
| `scene.apply` | edit | Aplicar una lista de operaciones de escena en una llamada |
| `object.call` | edit | Llamar un método de un nodo o recurso con args, soporta $res para recursos |
| `object.property.get` | read | Leer propiedad de un nodo o recurso |
| `object.property.set` | edit | Escribir propiedad de un nodo (undo/redo) o recurso |
| `godot.script.validate` | read | Validar sintaxis GDScript |
| `godot.script.write` | edit | Validar y escribir un archivo GDScript |
| `godot.signal.list` | read | Listar señales de un nodo |
| `godot.signal.connect` | edit | Conectar una señal a un método |
| `godot.signal.disconnect` | edit | Desconectar una señal |
| `godot.signal.emit` | edit | Emitir una señal con argumentos opcionales |
| `godot.asset.list` | read | Listar assets con estado de importación |
| `godot.asset.reimport` | edit | Reimportar un asset |
| `godot.resource.assign` | edit | Cargar un recurso y asignarlo a una propiedad |
| `godot.resource.create` | edit | Crear un recurso nuevo (Animation, Theme, SpriteFrames, ...) |
| `godot.project.files` | read | Listar archivos fuente del proyecto |
| `godot.project.settings.get` | read | Leer settings del proyecto |
| `godot.project.settings.set` | edit | Escribir y persistir settings del proyecto |
| `godot.run` | admin | Ejecutar el proyecto (con debugger) |
| `godot.input.send` | admin | Inyectar un input (tecla o acción) en el juego en ejecución |
| `godot.stop` | admin | Detener el proyecto |
| `godot.run.status` | read | Consultar si el proyecto está en ejecución |
| `godot.run.output` | read | Salida del juego en ejecución |
| `godot.runtime.tree` | read | Árbol remoto de escena del juego |
| `godot.runtime.metrics` | read | Monitores de rendimiento del juego |
| `debug.inspect` | read | Inspeccionar un objeto de runtime vía el debugger |
| `runtime.capture` | read | Captura de pantalla del juego |
| `plan.validate` | read | Validar la estructura de un plan |
| `plan.execute` | edit | Ejecutar pasos de un plan con referencias `$id.campo` |
| `skill.list` | read | Listar skills disponibles |
| `skill.get` | read | Detalle de una skill (tools que usa, conocimiento) |

## Configuración por cliente MCP

El token está en `tu-proyecto/.godot/osiris_ipc_token` (estable) y en `~/.local/share/godot/app_userdata/<Proyecto>/osiris_ipc_token`. Usa ruta absoluta si el binario no está en `PATH`; usa `--log-level debug` para diagnóstico (los logs van a stderr).

**Instalador automático** — registra Osiris en todos los CLIs de LLM instalados (opencode, codex, agy, Claude) de una vez:

```bash
scripts/osiris-install.sh          # instala en todos los CLIs detectados (idempotente)
scripts/osiris-install.sh --remove # quita de todos los CLIs detectados
```

Con una sesión activa, el CLI muestra Osiris en su barra de estado (ej: opencode muestra `⊙ N MCP` contando el server Osiris conectado).

**Cliente stdio genérico** (cualquier cliente MCP que lance un comando local) — usa el wrapper para auto-detectar el proyecto Godot y el perfil admin:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/ruta/a/osiris/scripts/osiris-mcp.sh"
    }
  }
}
```

**Claude Desktop** — `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/ruta/a/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876", "--capability-profile", "edit"]
    }
  }
}
```

**Cursor** — ajustes MCP del proyecto:

```json
{
  "mcpServers": {
    "osiris": {
      "command": "/ruta/a/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876"]
    }
  }
}
```

**VS Code (extensión mcp)** — `.mcp.json`:

```json
{
  "servers": {
    "osiris": {
      "command": "/ruta/a/osiris-server",
      "args": ["--ipc-addr", "127.0.0.1:9876"]
    }
  }
}
```

## Pruebas

```bash
cargo test --workspace                 # unitarias + integración + property
cargo bench -p osiris-ipc-client       # codec ~1µs; roundtrip real del bridge ~100ms
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                            # 0 vulnerabilidades conocidas
```

La suite cubre ciclo de vida, handshake, pureza de stdout, límites de tamaño, autorización por capacidades, idempotencia, reconexión IPC, health, salida de ejecución, planes, skills, observación de runtime y fuzzing por propiedades de los parsers.

## Seguridad

- El transporte MCP es **stdio**; sin superficie de red para la comunicación con el cliente.
- El transporte IPC es **TCP loopback** + archivo de token aleatorio (0600).
- Las rutas se validan: `..`, `//` y rutas absolutas son rechazadas.
- Los errores usan códigos de categoría estables; nunca exponen detalle interno.
- Los logs escriben a **stderr**; stdout está reservado para JSON-RPC.
- Las capacidades son **deny-by-default**; las mutaciones requieren cambio explícito de perfil.
- Auditoría completa: 0 vulnerabilidades conocidas, 0 bloques `unsafe`.

## Referencias

- [DECISIONS.md](DECISIONS.md): decisiones de arquitectura y deuda técnica.
- [COMPATIBILITY.md](COMPATIBILITY.md): matriz de versiones soportadas.
- [CONTRIBUTING.md](CONTRIBUTING.md): cómo contribuir.

## Licencia

Publicado bajo la [licencia Apache 2.0](LICENSE).

Copyright (c) 2026 Duna-tools.
