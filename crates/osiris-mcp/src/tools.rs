//! Registro de herramientas MCP expuestas por el servidor.
//!
//! El despacho delega a `osiris_application::handle_tool` (ADR-003). Este módulo
//! solo declara la superficie MCP: nombres, descripciones y esquemas.
//! La autorización por capacidades se aplica en `osiris-policy` (ADR-005).

use serde_json::{Value, json};

use crate::jsonrpc::RpcError;
use osiris_application;
use osiris_application::IdempotencyCache;
use osiris_ipc_client::IpcClient;
use osiris_policy::Profile;

/// `tools/call` con nombre desconocido o argumentos inválidos.
pub const INVALID_PARAMS: i64 = -32602;

/// Lista de herramientas para `tools/list`, filtradas por el perfil de capacidades.
#[must_use]
pub fn list(profile: Profile) -> Value {
    let all = all_tools();
    let visible: Vec<Value> = all
        .into_iter()
        .filter(|t| osiris_policy::visible_in(profile, t["name"].as_str().unwrap_or("")))
        .collect();
    json!({ "tools": visible })
}

#[allow(clippy::too_many_lines)]
fn all_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "osiris_diagnose",
            "title": "Diagnóstico del servidor",
            "description": "Devuelve nombre, versión y versiones de protocolo soportadas.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot_diagnose",
            "title": "Diagnóstico Godot",
            "description": "Devuelve la versión de Godot y el nombre del proyecto.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.active",
            "title": "Escena activa",
            "description": "Ruta, nombre raíz y número de nodos de la escena activa en el editor.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.nodes",
            "title": "Nodos de la escena",
            "description": "Lista nodos con profundidad y paginación.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath relativo." },
                    "depth": { "type": "integer", "description": "Profundidad máxima." },
                    "max_nodes": { "type": "integer", "description": "Máximo de nodos." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.properties",
            "title": "Propiedad de nodo",
            "description": "Lee una propiedad de un nodo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." },
                    "property": { "type": "string", "description": "Nombre de la propiedad." }
                },
                "required": ["property"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.resources",
            "title": "Recursos de nodo",
            "description": "Lista los recursos referenciados por un nodo.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "additionalProperties": false }
        }),
        json!({
            "name": "godot.editor.errors",
            "title": "Errores del editor",
            "description": "Lista errores y advertencias. Godot 4 no expone API pública.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.create_node",
            "title": "Crear nodo",
            "description": "Crea un nodo en la escena activa. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "type": {"type":"string"}, "name": {"type":"string"} }, "required": ["path","type","name"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.remove_node",
            "title": "Eliminar nodo",
            "description": "Elimina un nodo con Undo/Redo. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"} }, "required": ["path"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.set_property",
            "title": "Cambiar propiedad",
            "description": "Cambia una propiedad de un nodo con Undo/Redo. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "property": {"type":"string"}, "value": {} }, "required": ["path","property","value"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.save",
            "title": "Guardar escena",
            "description": "Guarda la escena activa. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.run",
            "title": "Ejecutar proyecto",
            "description": "Inicia la ejecución del proyecto o de una escena específica. Requiere perfil admin.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scene": { "type": "string", "description": "Ruta de la escena opcional a ejecutar (ej: res://main.tscn)." },
                    "timeout_secs": { "type": "integer", "description": "Tiempo máximo opcional de ejecución antes de auto-detención." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.stop",
            "title": "Detener proyecto",
            "description": "Detiene la ejecución del proyecto. Requiere perfil admin.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.run.status",
            "title": "Estado de ejecución",
            "description": "Devuelve si el proyecto está en ejecución. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.assert.property",
            "title": "Verificar propiedad",
            "description": "Compara el valor de una propiedad con un esperado. Devuelve match (bool) y el valor real.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." },
                    "property": { "type": "string", "description": "Nombre de la propiedad." },
                    "expected": { "description": "Valor esperado." }
                },
                "required": ["property"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.script.validate",
            "title": "Validar sintaxis de script",
            "description": "Valida la sintaxis de un código GDScript antes de escribirlo en disco.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Código GDScript a validar." }
                },
                "required": ["code"],
                "additionalProperties": false
            }
        }),
    ]
}

/// Ejecuta `tools/call`. `ipc` es la conexión al puente, si existe.
///
/// # Errors
/// `RpcError` con `INVALID_PARAMS` si falta `params.name` o la herramienta es
/// desconocida (spec MCP: error de protocolo, no `isError`).
pub fn call(
    params: &Value,
    ipc: Option<&mut IpcClient>,
    profile: Profile,
    cache: &mut IdempotencyCache,
) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "params.name requerido"))?;
    match name {
        "osiris_diagnose"
        | "godot_diagnose"
        | "godot.scene.active"
        | "godot.scene.nodes"
        | "godot.scene.properties"
        | "godot.scene.resources"
        | "godot.editor.errors"
        | "godot.scene.create_node"
        | "godot.scene.remove_node"
        | "godot.scene.set_property"
        | "godot.scene.save"
        | "godot.run"
        | "godot.stop"
        | "godot.run.status"
        | "godot.assert.property"
        | "godot.script.validate" => {
            let args = params.get("arguments").unwrap_or(&Value::Null);
            osiris_application::handle_tool(name, args, ipc, profile, cache)
                .map_err(|e| RpcError::from_domain(&e))
        }
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            format!("herramienta desconocida: {name}"),
        )),
    }
}
