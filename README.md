# Osiris — Godot MCP Server

A local [Model Context Protocol (MCP)](https://spec.modelcontextprotocol.io) server that inspects, modifies and controls Godot projects. Built in Rust with a GDScript bridge plugin.

Named after the Egyptian god of the underworld who judged the dead — this server audits and transforms your Godot scenes.

[English](#english) · [Español](#español)

---

## English

Osiris is an external MCP server that talks to a running Godot editor through a lightweight IPC bridge. MCP clients (Claude Desktop, Cursor, VS Code) launch the server; the server connects to the Godot editor's plugin via TCP loopback.

### Features

- **Scene inspection**: active scene path, node tree with pagination, properties and resources
- **Node properties**: read any node property with automatic Variant→JSON coercion (Vector, Color, Transform, etc.)
- **Scene modification** (permission-gated): create, remove, rename nodes; change properties with undo/redo
- **Project control**: save scenes, run/stop projects, check run status
- **Capability profiles**: `read` (default), `edit` (mutations), `admin` (project execution)
- **Protocol security**: stdio for MCP (zero network surface), TCP loopback + token for IPC, path traversal protection

### Architecture

```
MCP client  ←stdio→  osiris-server (Rust)  ←TCP loopback→  OsirisBridge (GDScript)  ←→  Godot Editor
```

- **`osiris-domain`**: stable error model with typed codes and retry policies
- **`osiris-application`**: tool orchestration, capability checks, idempotency cache
- **`osiris-mcp`**: JSON-RPC 2.0 server, stdio transport, tool registry
- **`osiris-policy`**: deny-by-default capabilities (read/edit/admin)
- **`osiris-ipc-contract`**: typed, versioned NDJSON protocol over TCP
- **`osiris-ipc-client`**: reconnect-capable IPC client with timeouts
- **`godot/osiris-bridge`**: EditorPlugin that bridges IPC to Godot's EditorInterface

### Requirements

- **Rust** 1.97+ (edition 2024)
- **Godot** 4.x (tested on 4.7.1)
- Linux (loopback TCP), macOS and Windows should work

### Quick start

```bash
# Build
cargo build --release

# Enable the bridge in your Godot project
cd your-godot-project
mkdir -p addons
ln -s /path/to/osiris/godot/osiris-bridge addons/osiris-bridge
# Enable the plugin in Project Settings → Plugins

# Start Godot and the server
godot --editor --path your-godot-project &
TOKEN=$(cat ~/.local/share/godot/app_userdata/YourProject/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

### Configuration

The binary reads configuration with precedence: **CLI args > env vars > TOML file > defaults**.

```
osiris-server [options]

  --config <path>           TOML config file (or OSIRIS_CONFIG)
  --bind <addr>             Listen address [default: 127.0.0.1:3000]
  --log-level <level>       Log level [default: info]
  --ipc-addr <addr>         Godot bridge address (or OSIRIS_IPC_ADDR)
  --ipc-token-file <path>   Bridge token file (or OSIRIS_IPC_TOKEN_FILE)
  --capability-profile <p>  read | edit | admin [default: read]
  -h, --help
```

### Tools

| Tool | Profile | Description |
|---|---|---|
| `osiris_diagnose` | read | Server name, version, protocol versions |
| `godot_diagnose` | read | Godot version + project name |
| `godot.scene.active` | read | Active scene path, root name, node count |
| `godot.scene.nodes` | read | Node tree (depth, pagination) |
| `godot.scene.properties` | read | Read a node property |
| `godot.scene.resources` | read | List resources referenced by a node |
| `godot.editor.errors` | read | Editor errors (placeholder — Godot 4 API limitation) |
| `godot.scene.create_node` | edit | Create a node (undo/redo) |
| `godot.scene.remove_node` | edit | Remove a node |
| `godot.scene.set_property` | edit | Change a property (undo/redo) |
| `godot.scene.save` | edit | Save active scene to disk |
| `godot.run` | admin | Run project |
| `godot.stop` | admin | Stop project |
| `godot.run.status` | read | Check if project is running |
| `godot.run.output` | read | Get output (log) of the running project |
| `godot.assert.property` | read | Assert property equals expected value |
| `godot.script.validate` | read | Validate GDScript syntax before writing it |

### Running tests

```bash
cargo test --workspace                 # unit + integration + property tests (21 contract tests)
cargo bench -p osiris-ipc-client       # IPC benchmarks (connect ~71µs, roundtrip ~41ms, codec ~0.2-0.9µs)
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                            # 0 known vulnerabilities
```

The suite covers: lifecycle, handshake, stdout purity (JSON-RPC only on stdout),
size limits, capability authorization, idempotency, IPC reconnect (UC-050),
health, run output, and property-based fuzzing of the parsers.

### Status

All roadmap phases A–F are implemented. Hardening (fuzzing, benchmarks, security
audit, codec decision) and the vault-required gaps (RF-003/011/014/028/034/035/041,
RNF-003/050, UC-050 reconnect) are closed. See `DECISIONS.md` for architecture
decisions and `COMPATIBILITY.md` for the version matrix. Deferred until a real
release: SBOM/signing, plugin SDK, HTTP sessions, metrics exporters.

### Security

- MCP transport is **stdio** — no network surface for client communication
- IPC transport is **TCP loopback** + random token file (0600)
- Paths are validated: `..`, `//`, and absolute paths are rejected
- Error messages use stable category codes — never expose internal details
- Logs write to **stderr** — stdout is reserved for the JSON-RPC protocol
- Capabilities are **deny-by-default**: mutations require explicit profile upgrade
- Full dependency audit: 0 known vulnerabilities, 0 `unsafe` blocks

### Per-client MCP setup

The server runs over stdio; each client configures it differently. The binary flags are the same for all (`osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <token> --capability-profile edit`). The token lives at `~/.local/share/godot/app_userdata/<Project>/osiris_ipc_token`.

**Claude Desktop** — add to `claude_desktop_config.json`:

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

**Cursor** — in the project MCP settings:

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

**VS Code (mcp extension)** — in `.mcp.json`:

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

Note: use an absolute path if the binary is not on `PATH`. For diagnostics: `--log-level debug` (logs go to stderr).

See `COMPATIBILITY.md` for the supported version matrix.

---

## Español

Osiris es un servidor MCP externo que se comunica con el editor Godot en ejecución mediante un puente IPC ligero. Los clientes MCP (Claude Desktop, Cursor, VS Code) lanzan el servidor por stdio; el servidor se conecta al plugin del editor Godot vía TCP loopback.

### Funcionalidades

- **Inspección de escenas**: ruta de la escena activa, árbol de nodos con paginación, propiedades y recursos
- **Propiedades de nodos**: lectura de cualquier propiedad con coerción automática Variant→JSON (Vector, Color, Transform, etc.)
- **Modificación de escenas** (con permisos): crear, eliminar, renombrar nodos; cambiar propiedades con undo/redo
- **Control de proyecto**: guardar escenas, ejecutar/detener proyectos, consultar estado de ejecución
- **Perfiles de capacidades**: `read` (por defecto), `edit` (mutaciones), `admin` (ejecución de proyecto)
- **Seguridad del protocolo**: stdio para MCP (sin superficie de red), TCP loopback + token para IPC, protección contra path traversal

### Arquitectura

```
Cliente MCP  ←stdio→  osiris-server (Rust)  ←TCP loopback→  OsirisBridge (GDScript)  ←→  Editor Godot
```

- **`osiris-domain`**: modelo de errores estable con códigos tipados y políticas de reintento
- **`osiris-application`**: orquestación de herramientas, verificación de capacidades, caché de idempotencia
- **`osiris-mcp`**: servidor JSON-RPC 2.0, transporte stdio, registro de herramientas
- **`osiris-policy`**: capacidades deny-by-default (read/edit/admin)
- **`osiris-ipc-contract`**: protocolo NDJSON tipado y versionado sobre TCP
- **`osiris-ipc-client`**: cliente IPC con timeouts y capacidad de reconexión
- **`godot/osiris-bridge`**: EditorPlugin que conecta IPC con EditorInterface de Godot

### Requisitos

- **Rust** 1.97+ (edition 2024)
- **Godot** 4.x (probado en 4.7.1)
- Linux (TCP loopback), macOS y Windows deberían funcionar

### Inicio rápido

```bash
# Compilar
cargo build --release

# Activar el puente en tu proyecto Godot
cd tu-proyecto-godot
mkdir -p addons
ln -s /ruta/a/osiris/godot/osiris-bridge addons/osiris-bridge
# Activar el plugin en Ajustes del Proyecto → Plugins

# Iniciar Godot y el servidor
godot --editor --path tu-proyecto-godot &
TOKEN=$(cat ~/.local/share/godot/app_userdata/TuProyecto/osiris_ipc_token)
./target/release/osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <(echo "$TOKEN")
```

### Configuración

El binario lee la configuración con precedencia: **args CLI > variables de entorno > archivo TOML > valores por defecto**.

```
osiris-server [opciones]

  --config <ruta>           Archivo TOML de configuración (o OSIRIS_CONFIG)
  --bind <addr>             Dirección de escucha [defecto: 127.0.0.1:3000]
  --log-level <nivel>       Nivel de log [defecto: info]
  --ipc-addr <addr>         Dirección del puente Godot (o OSIRIS_IPC_ADDR)
  --ipc-token-file <ruta>   Archivo con el token (o OSIRIS_IPC_TOKEN_FILE)
  --capability-profile <p>  read | edit | admin [defecto: read]
  -h, --help
```

### Herramientas

| Herramienta | Perfil | Descripción |
|---|---|---|
| `osiris_diagnose` | read | Nombre, versión y versiones de protocolo del servidor |
| `godot_diagnose` | read | Versión de Godot + nombre del proyecto |
| `godot.scene.active` | read | Escena activa: ruta, nombre raíz, número de nodos |
| `godot.scene.nodes` | read | Árbol de nodos (profundidad, paginación) |
| `godot.scene.properties` | read | Leer una propiedad de un nodo |
| `godot.scene.resources` | read | Recursos referenciados por un nodo |
| `godot.editor.errors` | read | Errores del editor (placeholder — limitación de API de Godot 4) |
| `godot.scene.create_node` | edit | Crear un nodo (undo/redo) |
| `godot.scene.remove_node` | edit | Eliminar un nodo |
| `godot.scene.set_property` | edit | Cambiar una propiedad (undo/redo) |
| `godot.scene.save` | edit | Guardar escena activa en disco |
| `godot.run` | admin | Ejecutar proyecto |
| `godot.stop` | admin | Detener proyecto |
| `godot.run.status` | read | Consultar si el proyecto está en ejecución |
| `godot.run.output` | read | Salida (log) del proyecto ejecutado |
| `godot.assert.property` | read | Verificar que una propiedad coincide con un valor esperado |
| `godot.script.validate` | read | Validar sintaxis GDScript antes de escribirlo |

### Ejecutar pruebas

```bash
cargo test --workspace      # tests unitarios + integración + property (21 tests de contrato)
cargo bench -p osiris-ipc-client   # benchmarks IPC (connect ~71µs, roundtrip ~41ms, codec ~0.2-0.9µs)
cargo clippy --workspace --all-targets -- -D warnings
cargo audit                 # 0 vulnerabilidades conocidas
```
cargo audit                 # 0 vulnerabilidades conocidas
```

### Seguridad

- El transporte MCP es **stdio** — sin superficie de red para la comunicación con el cliente
- El transporte IPC es **TCP loopback** + archivo de token aleatorio (0600)
- Las rutas se validan: `..`, `//` y rutas absolutas son rechazadas
- Los mensajes de error usan códigos de categoría estables — nunca exponen detalles internos
- Los logs escriben a **stderr** — stdout está reservado para el protocolo JSON-RPC
- Las capacidades son **deny-by-default**: las mutaciones requieren cambio explícito de perfil
- Auditoría completa de dependencias: 0 vulnerabilidades conocidas, 0 bloques `unsafe`

### Estado

Las fases A–F del roadmap están implementadas. Endurecimiento (fuzzing,
benchmarks, auditoría de seguridad, decisión de codec) y las brechas del vault
(RF-003/011/014/028/034/035/041, RNF-003/050, reconexión UC-050) están cerradas.
Ver `DECISIONS.md` para decisiones de arquitectura y `COMPATIBILITY.md` para la
matriz de versiones. Diferido hasta un release real: SBOM/firmas, SDK de plugins,
sesiones HTTP, exportadores de métricas.

### Configuración por cliente MCP

El servidor se lanza por stdio; cada cliente lo configura de forma distinta. Los argumentos del binario son los mismos para todos (`osiris-server --ipc-addr 127.0.0.1:9876 --ipc-token-file <token> --capability-profile edit`). El token está en `~/.local/share/godot/app_userdata/<Proyecto>/osiris_ipc_token`.

**Claude Desktop** — añadir a `claude_desktop_config.json`:

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

**Cursor** — en MCP settings del proyecto:

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

**VS Code (mcp extension)** — en `.mcp.json`:

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

Nota: si el binario no está en el `PATH`, usar ruta absoluta. Para ver el diagnóstico: `--log-level debug` (los logs van a stderr).

Consulte `COMPATIBILITY.md` para la matriz de versiones soportadas.
