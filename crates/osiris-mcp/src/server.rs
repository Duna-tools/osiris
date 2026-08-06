//! Servidor MCP sobre stdio: bucle de lectura y despacho de métodos.
//!
//! Mensajes JSON-RPC delimitados por líneas (spec MCP stdio). Los logs van a
//! stderr para no contaminar stdout, que es el canal del protocolo.
//! Métodos de esta fase: `initialize` (negociación de versión), `ping` y
//! `tools/list` (vacío hasta T-022/T-023). Las notificaciones no responden.

use std::io::{self, BufRead, Write};

use osiris_domain::DomainError;
use serde_json::{Value, json};

use crate::jsonrpc::{INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, Request, Response, RpcError};

/// Versiones del protocolo MCP que este servidor acepta.
const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// Versión que se propone cuando el cliente pide una no soportada (spec MCP).
const LATEST_VERSION: &str = SUPPORTED_VERSIONS[0];

/// Bucle principal: lee líneas de stdin y escribe respuestas en stdout hasta EOF.
pub fn run_stdio() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line) {
            writeln!(stdout, "{}", response.to_json())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Procesa una línea JSON-RPC y devuelve la respuesta, si procede.
///
/// `None` significa notificación: el protocolo prohíbe responderla.
pub fn handle_line(line: &str) -> Option<Response> {
    match parse(line) {
        Ok(request) => dispatch(&request),
        Err(response) => Some(*response),
    }
}

fn parse(line: &str) -> Result<Request, Box<Response>> {
    let value: Value = serde_json::from_str(line).map_err(|_| {
        Box::new(Response::err(
            Value::Null,
            RpcError::new(PARSE_ERROR, "parse error"),
        ))
    })?;
    let id = value.get("id").cloned();
    let request: Request = serde_json::from_value(value).map_err(|_| {
        Box::new(Response::err(
            id.clone().unwrap_or(Value::Null),
            RpcError::new(INVALID_REQUEST, "invalid request"),
        ))
    })?;
    if request.jsonrpc != "2.0" {
        return Err(Box::new(Response::err(
            request.id.clone().unwrap_or(Value::Null),
            RpcError::new(INVALID_REQUEST, "invalid request"),
        )));
    }
    Ok(request)
}

fn dispatch(request: &Request) -> Option<Response> {
    if request.method.starts_with("notifications/") {
        return None;
    }
    let id = request.id.clone()?;
    match request.method.as_str() {
        "initialize" => Some(handle_initialize(id, request)),
        "ping" => Some(Response::ok(id, json!({}))),
        "tools/list" => Some(Response::ok(id, json!({ "tools": [] }))),
        _ => Some(Response::err(
            id,
            RpcError::new(METHOD_NOT_FOUND, "method not found"),
        )),
    }
}

fn handle_initialize(id: Value, request: &Request) -> Response {
    let requested = request
        .params
        .get("protocolVersion")
        .and_then(Value::as_str);
    let version = match requested {
        Some(v) if SUPPORTED_VERSIONS.contains(&v) => v,
        Some(_) => LATEST_VERSION,
        None => {
            return Response::err(
                id,
                RpcError::from_domain(&DomainError::Validation(
                    "params.protocolVersion requerido".to_string(),
                )),
            );
        }
    };
    Response::ok(
        id,
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "osiris", "version": env!("CARGO_PKG_VERSION") },
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(line: &str) -> Value {
        let response = handle_line(line).expect("debe responder");
        serde_json::from_str(&response.to_json()).unwrap()
    }

    #[test]
    fn initialize_echoes_supported_version() {
        let r = call(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "osiris");
        assert_eq!(r["result"]["capabilities"]["tools"], json!({}));
    }

    #[test]
    fn initialize_with_unsupported_version_offers_latest() {
        let r = call(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], LATEST_VERSION);
    }

    #[test]
    fn initialize_without_version_is_domain_validation_error() {
        let r = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        assert_eq!(r["error"]["code"], -32000);
        assert_eq!(r["error"]["data"]["code"], "validation");
        assert_eq!(r["error"]["data"]["retryable"], false);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let r = call(r#"{"jsonrpc":"2.0","id":9,"method":"nope/x"}"#);
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 9);
    }

    #[test]
    fn malformed_json_is_parse_error_with_null_id() {
        let r = call("esto no es json");
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn wrong_jsonrpc_version_is_invalid_request() {
        let r = call(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#);
        assert_eq!(r["error"]["code"], -32600);
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(handle_line(r#"{"jsonrpc":"2.0","method":"ping"}"#).is_none());
    }

    #[test]
    fn domain_error_never_leaks_internal_cause() {
        // FND-004: el payload interno no debe aparecer en el JSON externo.
        let e = DomainError::Internal("ruta secreta /home/x/token".to_string());
        let err = RpcError::from_domain(&e);
        let json = serde_json::to_string(&err).unwrap();
        assert!(!json.contains("ruta secreta"));
        assert!(!json.contains("/home/x/token"));
        assert_eq!(err.message, "internal");
        let e = DomainError::Godot("node /root/Player roto".to_string());
        let json = serde_json::to_string(&RpcError::from_domain(&e)).unwrap();
        assert!(!json.contains("/root/Player"));
    }
}
