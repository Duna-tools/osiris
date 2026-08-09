# Matriz de compatibilidad

Verificada contra `compatibility-policy.md` de la documentación. Estado a 2026-08-08.

| Dimensión | Estado |
|---|---|
| Protocolo MCP | `2025-06-18`, `2025-03-26`, `2024-11-05` (negociado en `initialize`; versión más reciente ofrecida si la del cliente no está soportada) |
| Contrato IPC | `PROTOCOL_VERSION = 1` (handshake en `hello`/`hello_ack`; `hello_error` en versión incompatible) |
| Godot | 4.x — validado en **4.7.1** (Linux x86_64). Versiones 4.3+ esperadas compatibles (API `EditorUndoRedoManager`, `StreamPeerTCP`, `EditorInterface`) |
| Linux | Arch Linux (desarrollo) — loopback TCP |
| Windows | No validado todavía |
| macOS | No validado todavía |
| Clientes MCP | Ver `README.md` §Configuración por cliente |
| Rust | edition 2024, toolchain stable |

## Herramientas públicas

El nombre y el esquema de cada herramienta forman parte del contrato. Cambios de nombre o esquema rompen compatibilidad y requieren versión nueva.

## Deprecación

Antes de retirar una herramienta, un campo o una versión de protocolo:

1. anunciar con al menos una versión menor de antelación;
2. ofrecer alternativa;
3. mantener errores con la misma categoría externa (`DomainError.code()`).

## Verificación

- Contract tests sobre el binario real: `cargo test -p osiris-cli --test contract` (21 tests).
- Benchmark IPC: `cargo bench -p osiris-ipc-client`.
