//! Servicios de aplicación: orquestación, autorización, sesiones, cancelación
//! y coordinación de transacciones (docs/05-architecture/architecture-description.md §3).
//!
//! Capa intermedia entre los adaptadores (MCP, IPC) y el dominio. Contiene la
//! lógica de negocio que no pertenece a ningún adaptador concreto.

use osiris_domain::DomainError;
use osiris_ipc_client::IpcClient;
use osiris_policy::Profile;
use serde_json::Value;

/// Despacha el `tools/call` hacia la herramienta correcta (local o puente).
///
/// `profile` determina las capacidades del cliente (ADR-005).
pub fn handle_tool(
    name: &str,
    params: &Value,
    ipc: Option<&mut IpcClient>,
    profile: Profile,
) -> Result<Value, DomainError> {
    osiris_policy::check(profile, name)?;
    match name {
        "osiris_diagnose" => Ok(osiris_diagnose()),
        "godot_diagnose" => ipc_tool(ipc, "get_version", Value::Null, |client, result| {
            serde_json::json!({
                "godot_version": client.godot_version,
                "godot_details": result,
            })
        }),
        "godot.scene.active" => ipc_tool(ipc, "get_active_scene", Value::Null, plain_result()),
        "godot.scene.nodes" => ipc_tool(ipc, "list_nodes", params.clone(), plain_result()),
        "godot.scene.properties" => ipc_tool(ipc, "get_property", params.clone(), plain_result()),
        "godot.scene.resources" => ipc_tool(ipc, "list_resources", params.clone(), plain_result()),
        "godot.editor.errors" => ipc_tool(ipc, "get_editor_errors", Value::Null, plain_result()),
        // Escritura: requieren capacidades (check en policy)
        "godot.scene.create_node" => ipc_tool(ipc, "create_node", params.clone(), plain_result()),
        "godot.scene.remove_node" => ipc_tool(ipc, "remove_node", params.clone(), plain_result()),
        "godot.scene.set_property" => ipc_tool(ipc, "set_property", params.clone(), plain_result()),
        "godot.scene.save" => ipc_tool(ipc, "save_scene", Value::Null, plain_result()),
        // Ejecución
        "godot.run" => ipc_tool(ipc, "run_project", Value::Null, plain_result()),
        "godot.stop" => ipc_tool(ipc, "stop_project", Value::Null, plain_result()),
        _ => Err(DomainError::Validation(format!(
            "herramienta desconocida: {name}"
        ))),
    }
}

fn osiris_diagnose() -> Value {
    let info = serde_json::json!({
        "name": "osiris",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersions": ["2025-06-18", "2025-03-26", "2024-11-05"],
        "transport": "stdio",
    });
    serde_json::json!({
        "content": [{ "type": "text", "text": info.to_string() }],
        "isError": false,
    })
}

fn plain_result() -> fn(&IpcClient, &Value) -> Value {
    |_, result| result.clone()
}

/// Ejecuta una orden en el puente y envuelve el resultado como texto MCP.
fn ipc_tool(
    ipc: Option<&mut IpcClient>,
    op: &str,
    params: Value,
    transform: impl FnOnce(&IpcClient, &Value) -> Value,
) -> Result<Value, DomainError> {
    let client = ipc.ok_or_else(|| {
        DomainError::Ipc("el puente Godot no está configurado (usa --ipc-addr)".to_string())
    })?;
    let result = client.call(op, params)?;
    let payload = transform(client, &result);
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": payload.to_string() }],
        "isError": false,
    }))
}
