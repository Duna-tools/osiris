//! Registro de herramientas MCP expuestas por el servidor.
//!
//! Hoy solo hay una herramienta de diagnóstico (T-023). Cuando lleguen las
//! herramientas Godot (Fase C+), el despacho pasará por `osiris-application`
//! y la autorización por `osiris-policy` (ADR-005); este módulo quedará como
//! la descripción de la superficie MCP (nombres, descripciones, esquemas).

use serde_json::{Value, json};

use crate::jsonrpc::RpcError;

/// `tools/call` con nombre desconocido o argumentos inválidos.
pub const INVALID_PARAMS: i64 = -32602;

/// Lista de herramientas para `tools/list` (esquemas JSON Schema, spec MCP).
pub fn list() -> Value {
    json!({
        "tools": [{
            "name": "osiris_diagnose",
            "title": "Diagnóstico del servidor",
            "description": "Devuelve nombre, versión y versiones de protocolo soportadas. Solo lectura; no requiere capacidades.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }]
    })
}

/// Ejecuta `tools/call`. Devuelve el `result` MCP con `content`/`isError`.
///
/// # Errores
/// `RpcError` con `INVALID_PARAMS` si falta `params.name` o la herramienta no
/// existe (spec MCP: error de protocolo, no `isError`).
pub fn call(params: &Value) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "params.name requerido"))?;
    match name {
        "osiris_diagnose" => Ok(diagnose()),
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            format!("herramienta desconocida: {name}"),
        )),
    }
}

fn diagnose() -> Value {
    let info = json!({
        "name": "osiris",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersions": ["2025-06-18", "2025-03-26", "2024-11-05"],
        "transport": "stdio",
    });
    json!({
        "content": [{ "type": "text", "text": info.to_string() }],
        "isError": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_declares_diagnose_with_schema() {
        let tools = &list()["tools"];
        let diag = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "osiris_diagnose")
            .expect("osiris_diagnose registrada");
        assert_eq!(diag["inputSchema"]["type"], "object");
        assert!(diag["description"].as_str().unwrap().len() > 10);
    }

    #[test]
    fn call_diagnose_returns_content() {
        let r = call(&json!({"name": "osiris_diagnose", "arguments": {}})).unwrap();
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
        let e = call(&json!({"name": "nope"})).unwrap_err();
        let e = format!("{:?}", e);
        assert!(e.contains("-32602"));
        assert!(call(&json!({})).is_err());
    }
}
