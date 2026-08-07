//! Registro de herramientas MCP expuestas por el servidor.
//!
//! El despacho delega a `osiris_application::handle_tool` (ADR-003). Este módulo
//! solo declara la superficie MCP: nombres, descripciones y esquemas.
//! La autorización por capacidades llegará con `osiris-policy` (ADR-005, Fase E).

use serde_json::{Value, json};

use crate::jsonrpc::RpcError;
use osiris_application;
use osiris_ipc_client::IpcClient;

/// `tools/call` con nombre desconocido o argumentos inválidos.
pub const INVALID_PARAMS: i64 = -32602;

/// Lista de herramientas para `tools/list` (esquemas JSON Schema, spec MCP).
pub fn list() -> Value {
    json!({
        "tools": [
            {
                "name": "osiris_diagnose",
                "title": "Diagnóstico del servidor",
                "description": "Devuelve nombre, versión y versiones de protocolo soportadas. Solo lectura; no requiere capacidades.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "godot_diagnose",
                "title": "Diagnóstico Godot",
                "description": "Devuelve la versión de Godot y el nombre del proyecto. Requiere conexión al puente (--ipc-addr). Solo lectura.",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }
        ]
    })
}

/// Ejecuta `tools/call`. `ipc` es la conexión al puente, si existe.
///
/// # Errores
/// `RpcError` con `INVALID_PARAMS` si falta `params.name` o la herramienta no
/// existe (spec MCP: error de protocolo, no `isError`).
pub fn call(params: &Value, ipc: Option<&mut IpcClient>) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "params.name requerido"))?;
    match name {
        "osiris_diagnose" | "godot_diagnose" => {
            let args = params.get("arguments").unwrap_or(&Value::Null);
            osiris_application::handle_tool(name, args, ipc).map_err(|e| RpcError::from_domain(&e))
        }
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            format!("herramienta desconocida: {name}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_declares_both_tools_with_schema() {
        let tools = &list()["tools"];
        let names: Vec<_> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"osiris_diagnose"));
        assert!(names.contains(&"godot_diagnose"));
    }

    #[test]
    fn call_diagnose_returns_content() {
        let r = call(&json!({"name": "osiris_diagnose", "arguments": {}}), None).unwrap();
        assert_eq!(r["isError"], false);
        assert!(
            r["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"osiris\"")
        );
    }

    #[test]
    fn unknown_tool_and_missing_name_are_invalid_params() {
        let e = call(&json!({"name": "nope"}), None).unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("herramienta desconocida"));
        assert!(call(&json!({}), None).is_err());
    }

    #[test]
    fn godot_diagnose_without_ipc_is_error() {
        let e = call(&json!({"name": "godot_diagnose", "arguments": {}}), None).unwrap_err();
        let e = format!("{e:?}");
        assert!(e.contains("ipc"));
    }
}
