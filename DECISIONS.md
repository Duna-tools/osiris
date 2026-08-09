# Registro de decisiones

Este archivo versionable referencia los ADRs (fuente de verdad en el vault
`osiris-documentacion/docs/06-decisions/adr/`) y registra las decisiones de
implementación tomadas durante el desarrollo de Osiris que no están en el vault.

## ADRs del vault (fuente de verdad)

| ADR | Estado | Decisión |
|---|---|---|
| ADR-001 | Aceptada | Rust para el núcleo. |
| ADR-002 | Aceptada provisional | Arquitectura híbrida (núcleo externo + puente Godot). |
| ADR-003 | Aceptada | Arquitectura hexagonal. |
| ADR-004 | Propuesta | IPC tipado y versionado. Confirmado por benchmark: JSON es suficiente (T-034). |
| ADR-005 | Aceptada | Seguridad por capacidades deny-by-default. |
| ADR-006 | Aceptada | Loopback por defecto. |
| ADR-007 | Aceptada | No mover el núcleo a GDExtension inicialmente. |
| ADR-008 | Aceptada | Matriz explícita de compatibilidad. |

## Decisiones de implementación

### D-001 — Transporte MCP: stdio, no HTTP (2026-08-04)

- **Contexto:** ADR-002 define el núcleo externo; el transporte MCP hacia el
  cliente quedaba abierto ("HTTP o transporte seleccionado").
- **Decisión:** stdio. El cliente MCP lanza el proceso (estándar local);
  cero superficie de red en el lado MCP. THR-001/THR-011 y la clase del
  CVE-2026-11624 de Beckett desaparecen por diseño.
- **Consecuencias:** un cliente por proceso; reconexión IPC cubierta por UC-050.
  HTTP loopback + token (T-024) queda diferido a necesidad real de multi-cliente.

### D-002 — IPC: TCP loopback + NDJSON JSON, puente escucha (2026-08-04)

- **Contexto:** ADR-004 Propuesta (alternativas: JSON, MessagePack/CBOR, TCP loopback).
- **Decisión:** TCP loopback + JSON por líneas. El puente escucha; el núcleo
  conecta y reconecta (el editor Godot sobrevive a los núcleos, que mueren con
  cada cliente MCP).
- **Evidencia:** benchmark T-034 — codec ~0.2-0.9 µs vs 41 ms roundtrip TCP
  (<0.005%); el formato no es el cuello de botella. JSON se mantiene.

### D-003 — Autenticación IPC: token por archivo (2026-08-04)

- **Decisión:** el puente genera un token aleatorio (32 bytes hex) y lo escribe
  en `user://osiris_ipc_token` (permisos 0600). El núcleo lo lee de archivo
  (`--ipc-token-file`). Comparación simple de strings: defensa de proceso local,
  no criptográfica (FND-008 documentado).

### D-004 — Modelo síncrono en el núcleo (2026-08-08)

- **Contexto:** concurrency-model.md propone runtime async.
- **Decisión:** el núcleo MCP procesa una solicitud a la vez (síncrono) sobre
  stdio. El concurrency-model.md es aspiracional; sin cliente concurrente real,
  async es complejidad sin beneficio (YAGNI).
- **Consecuencias:** RF-004 (cancelación de llamada en vuelo) queda bloqueado;
  RF-035 se satisface con timeout IPC configurable (`--ipc-timeout-ms`).

### D-005 — Presupuesto temporal en el puente (2026-08-08)

- **Decisión:** `MAX_POLL_TIME_MSEC = 5` y `MAX_MESSAGES_PER_POLL = 10` en el
  sondeo del bridge (RNF-003). Una operación potencialmente larga no bloquea el
  editor: se procesa un lote acotado por frame.

### D-006 — Undo/Redo vía EditorUndoRedoManager (2026-08-08)

- **Contexto:** Godot 4.3+ eliminó `EditorInterface.get_undo_redo()`.
- **Decisión:** el puente recibe el `EditorUndoRedoManager` desde el
  `EditorPlugin.get_undo_redo()` y lo usa para mutaciones (create/set).

### D-007 — MCP 2026-07-28 diferido (2026-08-08)

- **Contexto:** la spec MCP 2026-07-28 es un cambio breaking: elimina
  `initialize`/`initialized` (estado de sesión), añade `server/discover`,
  resultados cacheables (`ttlMs`/`cacheScope`) y routing por headers HTTP
  (ADR-002 del vault nuevo).
- **Decisión:** nuestro transporte es stdio; la versión del protocolo viaja en
  headers HTTP en la spec nueva. Sin un cliente real 2026-07-28 para probar el
  comportamiento stateless, migrar es especulativo y mentiría al cliente
  (aceptar la versión sin implementar el modo stateless).
- **Consecuencias:** el servidor sigue en baseline 2025-06-18. Migrar exige:
  quitar el gate `initialized`, `server/discover`, `ttlMs`/`cacheScope` y
  pruebas con cliente real. No se añade `2026-07-28` a `SUPPORTED_VERSIONS`
  hasta entonces.

## Deuda técnica

| ID | Ítem | Severidad | Condición de cierre |
|---|---|---|---|
| TD-001 | Contrato IPC provisional JSON | Media | Resuelto: T-034 confirma JSON (D-002) |
| TD-002 | Matriz limitada de Godot | Alta | Ampliar antes de release estable |
| TD-003 | Política de plugins no definida | Media | RFC antes de habilitar (T-080) |
| TD-004 | Persistencia de journal no seleccionada | Alta | Prototipo en fase E |
| TD-005 | Clientes MCP en CI | Media | Harness por cliente (sin release aún) |
