# Contributing

Gracias por contribuir a Osiris. El proyecto se guía por la documentación de
ingeniería en el vault `osiris-godot-mcp-docs` (Onion Architecture, ADRs) y por
las decisiones en [DECISIONS.md](DECISIONS.md).

## Cómo contribuir

1. Crea una rama con prefijo `t/` (tema), p.ej. `t/RF-12-capabilities`.
2. Haz cambios pequeños y enfocados: una rama, una idea.
3. Antes de abrir el PR, pasa el guantelete:
   ```sh
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
4. Si tocas el bridge GDScript, verifica contra un Godot real (4.7+):
   el addon en `godot/osiris-bridge/` debe cargar sin SCRIPT ERROR y el ciclo
   `godot.run` → `godot.editor.errors` → `osiris.run.history` debe funcionar.
5. Abre el PR contra `main` describiendo qué resuelve y cómo lo verificaste.

## Convenciones

- **Rust**: edition 2024, clippy con `-D warnings`, sin `unsafe`.
- **GDScript**: el bridge es `@tool` y `class_name`; los IDs de respuesta IPC se
  serializan como enteros (`int(id)` en `_ok`/`_error`).
- **Tools**: cada tool es un `ToolSpec` en `osiris-application` + un schema en
  `osiris-mcp` + op en el bridge + un contract test. Los nombres y efectos deben
  coincidir entre los tres.
- **Errores**: usa `DomainError` del dominio; el mensaje externo es solo el
  código estable, el detalle interno va a logs.

## Documentación

- Los cambios de arquitectura van a `DECISIONS.md` (decisiones) y al vault.
- Los hallazgos verificados en el source de Godot van a `17_evaluacion_source_godot.md`
  y `18_errores_godot.md`.

## Reportar bugs

Incluye: versión de Godot, pasos para reproducir, la salida de
`osiris conformance` y el log de stderr del servidor (si aplica).
