//! Servicios de aplicación: orquestación, autorización, sesiones, cancelación
//! y coordinación de transacciones (docs/05-architecture/architecture-description.md §3).
//!
//! Capa intermedia entre los adaptadores (MCP, IPC) y el dominio. Contiene la
//! lógica de negocio que no pertenece a ningún adaptador concreto.

use osiris_domain::DomainError;
use osiris_ipc_client::IpcClient;
use serde_json::Value;

/// Despacha el `tools/call` hacia la herramienta correcta (local o puente).
///
/// `ipc` es opcional: si la herramienta requiere Godot y no hay conexión,
/// devuelve `Ipc`. En el futuro pasará por el motor de políticas (ADR-005).
pub fn handle_tool(
    name: &str,
    _params: &Value,
    ipc: Option<&mut IpcClient>,
) -> Result<Value, DomainError> {
    match name {
        "osiris_diagnose" => Ok(osiris_diagnose()),
        "godot_diagnose" => godot_diagnose(ipc),
        _ => Err(DomainError::Validation(format!(
            "herramienta desconocida: {name}"
        ))),
    }
}

fn osiris_diagnose() -> Value {
    // mover de tools.rs para tener una sola fuente
    serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::json!({
                "name": "osiris",
                "version": env!("CARGO_PKG_VERSION"),
                "protocolVersions": ["2025-06-18", "2025-03-26", "2024-11-05"],
                "transport": "stdio"
            }).to_string(),
        }],
        "isError": false,
    })
}

fn godot_diagnose(ipc: Option<&mut IpcClient>) -> Result<Value, DomainError> {
    let client = ipc.ok_or_else(|| {
        DomainError::Ipc("el puente Godot no está configurado (usa --ipc-addr)".to_string())
    })?;
    let godot_deep = client.call("get_version", Value::Null)?;
    Ok(serde_json::json!({
        "content": [{
            "type": "text",
            "text": serde_json::json!({
                "godot_version": client.godot_version,
                "godot_details": godot_deep,
            }).to_string(),
        }],
        "isError": false,
    }))
}
