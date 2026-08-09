<div align="center">

<a id="english"></a>

# Osiris

### MCP server for Godot projects

English | [Español](#espanol)

![rust](https://img.shields.io/badge/language-rust-orange)
![edition](https://img.shields.io/badge/edition-2024-blue)
![os](https://img.shields.io/badge/os-linux%2C%20win%2C%20mac-lightgrey)
![godot](https://img.shields.io/badge/godot-4.x-green)
![license](https://img.shields.io/badge/license-MIT-blue)

</div>

**Osiris is a local MCP (Model Context Protocol) server that inspects, modifies and controls Godot projects. A Rust core talks to a GDScript bridge running inside the Godot editor over TCP loopback, giving MCP clients (Claude Desktop, Cursor, VS Code) typed access to scenes, properties, execution and safety policy.**

## Key Features

### Scene Inspection
> Read-only access to the editor state.

- **Active scene**: path, root name and node count.
- **Node tree**: paginated traversal with depth limit and node cap.
- **Properties and resources**: read any node property with automatic Variant to JSON coercion (Vector, Color, Transform) and list referenced resources.

### Safe Modification
> Every mutation is reversible, permissioned and undo-aware.

- **Create, remove and edit nodes**: operations go through `EditorUndoRedoManager`; Ctrl+Z undoes them.
- **Capability profiles**: `read` (default), `edit` (mutations), `admin` (project execution); denied by default.
- **Idempotency**: repeated reads are cached (FIFO, 128 entries); mutations are never auto-retried.
- **Script validation**: GDScript syntax is validated before it reaches disk.

### Project Control
> Run, observe and assert.

- **Run and stop** the project with captured output (`--log-file`), process kill by PID, and status queries.
- **Assertions** on observable node state.
- **Reconnection** (UC-050): broken IPC channels reconnect for reads; mutations never auto-retry.

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

Enable the plugin in Project Settings, Plugins. Start the editor.

### Step 3: Run the server

```bash
godot --editor --path your-godot-project &
TOKEN=$(cat ~/.local/share/godot/app_userdata/YourProject/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

Default profile is `read`. Use `--capability-profile edit` (mutations) or `admin` (project execution).

## Configuration

Precedence: **CLI args > env vars > TOML file > defaults**.

```
osiris-server [options]

  --config <path>           TOML config file (or OSIRIS_CONFIG)
  --bind <addr>             Listen address [default: 127.0.0.1:3000]
  --log-level <level>       Log level [default: info]
  --ipc-addr <addr>         Godot bridge address (or OSIRIS_IPC_ADDR)
  --ipc-token-file <path>   Bridge token file (or OSIRIS_IPC_TOKEN_FILE)
  --ipc-timeout-ms <n>      IPC operation timeout (or OSIRIS_IPC_TIMEOUT_MS) [default: 3000]
  --capability-profile <p>  read | edit | admin [default: read]
  -h, --help
```

## Tools

| Tool | Profile | Description |
|---|---|---|
| `osiris_diagnose` | read | Server name, version, protocol versions |
| `godot_diagnose` | read | Godot version + project name |
| `godot.scene.active` | read | Active scene path, root name, node count |
| `godot.scene.nodes` | read | Node tree (depth, pagination) |
| `godot.scene.properties` | read | Read a node property |
| `godot.scene.resources` | read | List resources referenced by a node |
| `godot.editor.errors` | read | Editor errors (placeholder; Godot 4 API limitation) |
| `godot.scene.create_node` | edit | Create a node (undo/redo) |
| `godot.scene.remove_node` | edit | Remove a node |
| `godot.scene.set_property` | edit | Change a property (undo/redo) |
| `godot.scene.save` | edit | Save active scene to disk |
| `godot.run` | admin | Run project |
| `godot.stop` | admin | Stop project |
| `godot.run.status` | read | Check if project is running |
| `godot.run.output` | read | Get output (log) of the running project |
| `godot.assert.property` | read | Assert property equals expected value |
| `godot.script.validate` | read | Validate GDScript syntax |

## Per-client MCP setup

The token lives at `~/.local/share/godot/app_userdata/<Project>/osiris_ipc_token`. Use an absolute path if the binary is not on `PATH`; use `--log-level debug` for diagnostics (logs go to stderr).

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
cargo test --workspace                 # unit + integration + property tests (21 contract tests)
cargo bench -p osiris-ipc-client       # connect ~71µs, roundtrip ~41ms, codec ~0.2-0.9µs
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                            # 0 known vulnerabilities
```

The suite covers lifecycle, handshake, stdout purity, size limits, capability authorization, idempotency, IPC reconnect, health, run output, and property-based fuzzing of the parsers.

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

## License

Released under the [MIT license](LICENSE).

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
![license](https://img.shields.io/badge/license-MIT-blue)

</div>

**Osiris es un servidor MCP (Model Context Protocol) local que inspecciona, modifica y controla proyectos Godot. Un núcleo Rust se comunica con un puente GDScript dentro del editor Godot vía TCP loopback, dando a los clientes MCP (Claude Desktop, Cursor, VS Code) acceso tipado a escenas, propiedades, ejecución y política de seguridad.**

## Características

### Inspección de escenas
> Acceso de solo lectura al estado del editor.

- **Escena activa**: ruta, nombre raíz y número de nodos.
- **Árbol de nodos**: recorrido paginado con límite de profundidad y tope de nodos.
- **Propiedades y recursos**: lectura de cualquier propiedad con coerción automática Variant a JSON (Vector, Color, Transform) y listado de recursos referenciados.

### Modificación segura
> Toda mutación es reversible, con permisos y con undo.

- **Crear, eliminar y editar nodos**: las operaciones pasan por `EditorUndoRedoManager`; Ctrl+Z las deshace.
- **Perfiles de capacidades**: `read` (por defecto), `edit` (mutaciones), `admin` (ejecución); negado por defecto.
- **Idempotencia**: las lecturas repetidas se cachean (FIFO, 128 entradas); las mutaciones nunca se reintentan solas.
- **Validación de scripts**: la sintaxis GDScript se valida antes de llegar al disco.

### Control de proyecto
> Ejecutar, observar y verificar.

- **Ejecutar y detener** el proyecto con salida capturada (`--log-file`), matar proceso por PID y consultar estado.
- **Aserciones** sobre el estado observable de nodos.
- **Reconexión** (UC-050): canales IPC rotos se reconectan para lecturas; las mutaciones nunca se reintentan solas.

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

Activa el plugin en Ajustes del Proyecto, Plugins. Inicia el editor.

### Paso 3: Ejecutar el servidor

```bash
godot --editor --path tu-proyecto-godot &
TOKEN=$(cat ~/.local/share/godot/app_userdata/TuProyecto/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

El perfil por defecto es `read`. Usa `--capability-profile edit` (mutaciones) o `admin` (ejecución).

## Configuración

Precedencia: **args CLI > variables de entorno > archivo TOML > valores por defecto**.

```
osiris-server [opciones]

  --config <ruta>           Archivo TOML de configuración (o OSIRIS_CONFIG)
  --bind <addr>             Dirección de escucha [defecto: 127.0.0.1:3000]
  --log-level <nivel>       Nivel de log [defecto: info]
  --ipc-addr <addr>         Dirección del puente Godot (o OSIRIS_IPC_ADDR)
  --ipc-token-file <ruta>   Archivo con el token (o OSIRIS_IPC_TOKEN_FILE)
  --ipc-timeout-ms <n>      Timeout de operaciones IPC (o OSIRIS_IPC_TIMEOUT_MS) [defecto: 3000]
  --capability-profile <p>  read | edit | admin [defecto: read]
  -h, --help
```

## Herramientas

| Herramienta | Perfil | Descripción |
|---|---|---|
| `osiris_diagnose` | read | Nombre, versión y versiones de protocolo |
| `godot_diagnose` | read | Versión de Godot + nombre del proyecto |
| `godot.scene.active` | read | Escena activa: ruta, nombre raíz, nodos |
| `godot.scene.nodes` | read | Árbol de nodos (profundidad, paginación) |
| `godot.scene.properties` | read | Leer una propiedad de un nodo |
| `godot.scene.resources` | read | Recursos referenciados por un nodo |
| `godot.editor.errors` | read | Errores del editor (placeholder; limitación API Godot 4) |
| `godot.scene.create_node` | edit | Crear un nodo (undo/redo) |
| `godot.scene.remove_node` | edit | Eliminar un nodo |
| `godot.scene.set_property` | edit | Cambiar una propiedad (undo/redo) |
| `godot.scene.save` | edit | Guardar escena activa |
| `godot.run` | admin | Ejecutar proyecto |
| `godot.stop` | admin | Detener proyecto |
| `godot.run.status` | read | Consultar si el proyecto está en ejecución |
| `godot.run.output` | read | Salida (log) del proyecto ejecutado |
| `godot.assert.property` | read | Verificar que una propiedad coincide con un valor esperado |
| `godot.script.validate` | read | Validar sintaxis GDScript |

## Configuración por cliente MCP

El token está en `~/.local/share/godot/app_userdata/<Proyecto>/osiris_ipc_token`. Usa ruta absoluta si el binario no está en `PATH`; usa `--log-level debug` para diagnóstico (los logs van a stderr).

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
cargo test --workspace                 # unitarias + integración + property (21 tests de contrato)
cargo bench -p osiris-ipc-client       # connect ~71µs, roundtrip ~41ms, codec ~0.2-0.9µs
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                            # 0 vulnerabilidades conocidas
```

La suite cubre ciclo de vida, handshake, pureza de stdout, límites de tamaño, autorización por capacidades, idempotencia, reconexión IPC, health, salida de ejecución y fuzzing por propiedades de los parsers.

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

## Licencia

Publicado bajo la [licencia MIT](LICENSE).
