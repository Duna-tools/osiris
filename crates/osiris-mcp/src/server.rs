//! Servidor MCP sobre stdio: bucle de lectura y despacho de métodos.
//!
//! Mensajes JSON-RPC delimitados por líneas (spec MCP stdio). Los logs van a
//! stderr para no contaminar stdout, que es el canal del protocolo.
//!
//! Ciclo de vida (spec MCP): antes de `initialize` solo se aceptan
//! `initialize` y `ping`; el resto recibe -32002. `initialize` es una sola
//! vez. Las notificaciones no responden nunca. En stdio la sesión ES la vida
//! del proceso; expiración e inactividad se difieren al futuro modo HTTP.
//!
//! Límites (RF-035/RNF-001): mensaje máximo [`MAX_MESSAGE_BYTES`]; el
//! procesamiento es secuencial (un mensaje a la vez), que es la estrategia de
//! backpressure en stdio. El límite de duración y cancelación está pendiente
//! de las herramientas de larga duración (run/stop, T-060) y se fijará con
//! un presupuesto de hilo principal cuando se midan tiempos reales.

use std::io::{self, BufRead, Write};

use osiris_domain::DomainError;
use serde_json::{Value, json};

use crate::jsonrpc::{INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, Request, Response, RpcError};
use osiris_application::IdempotencyCache;
use osiris_ipc_client::IpcClient;
use osiris_policy::Profile;

/// Versiones del protocolo MCP que este servidor acepta.
const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
/// Versión que se propone cuando el cliente pide una no soportada (spec MCP).
const LATEST_VERSION: &str = SUPPORTED_VERSIONS[0];

/// Petición antes de completar `initialize`.
pub const NOT_INITIALIZED: i64 = -32002;

/// Tamaño máximo de un mensaje JSON-RPC (bytes de la línea).
///
/// Nota: el peer de stdio es el proceso cliente que nos lanzó (confianza
/// local); este tope acota memoria por mensaje como defensa básica, no contra
/// un peer hostil (eso corresponderá al modo HTTP).
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// Estado del servidor: el ciclo de vida MCP sobre stdio.
#[derive(Debug)]
pub struct Server {
    initialized: bool,
    pub ipc: Option<IpcClient>,
    pub profile: Profile,
    cache: IdempotencyCache,
}

impl Server {
    #[must_use]
    pub fn new(ipc: Option<IpcClient>, profile: Profile) -> Self {
        Self {
            initialized: false,
            ipc,
            profile,
            cache: IdempotencyCache::new(),
        }
    }

    /// Procesa una línea JSON-RPC y devuelve la respuesta, si procede.
    ///
    /// `None` significa notificación: el protocolo prohíbe responderla.
    pub fn handle_line(&mut self, line: &str) -> Option<Response> {
        if line.len() > MAX_MESSAGE_BYTES {
            return Some(Response::err(
                Value::Null,
                RpcError::new(INVALID_REQUEST, "message too large"),
            ));
        }
        match parse(line) {
            Ok(request) => self.dispatch(&request),
            Err(response) => Some(*response),
        }
    }

    fn dispatch(&mut self, request: &Request) -> Option<Response> {
        if request.method.starts_with("notifications/") {
            return None;
        }
        let id = request.id.clone()?;
        tracing::debug!(req_id = %id, method = %request.method, "solicitud recibida");
        match request.method.as_str() {
            "initialize" => Some(self.handle_initialize(id, request)),
            // El spec MCP permite pings antes de completar la inicialización.
            "ping" => Some(Response::ok(id, json!({}))),
            _ if !self.initialized => Some(Response::err(
                id,
                RpcError::new(
                    NOT_INITIALIZED,
                    "server not initialized: envía initialize primero",
                ),
            )),
            "tools/list" => Some(Response::ok(id, crate::tools::list(self.profile))),
            "health" => Some(self.handle_health(id)),
            "tools/call" => Some(
                match crate::tools::call(
                    &request.params,
                    self.ipc.as_mut(),
                    self.profile,
                    &mut self.cache,
                ) {
                    Ok(result) => Response::ok(id, result),
                    Err(e) => Response::err(id, e),
                },
            ),
            _ => Some(Response::err(
                id,
                RpcError::new(METHOD_NOT_FOUND, "method not found"),
            )),
        }
    }

    fn handle_health(&self, id: Value) -> Response {
        Response::ok(
            id,
            json!({
                "ok": true,
                "version": env!("CARGO_PKG_VERSION"),
                "profile": format!("{:?}", self.profile).to_lowercase(),
                "godot_connected": self.ipc.is_some(),
            }),
        )
    }

    fn handle_initialize(&mut self, id: Value, request: &Request) -> Response {
        if self.initialized {
            return Response::err(
                id,
                RpcError::new(INVALID_REQUEST, "initialize ya completado"),
            );
        }
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
        self.initialized = true;
        Response::ok(
            id,
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "osiris", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
    }
}

/// Main loop: reads stdin lines and writes responses to stdout until EOF.
///
/// # Errors
/// Devuelve `Err` por error de E/S al leer stdin o escribir stdout.
pub fn run_server(server: &mut Server) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(&line) {
            writeln!(stdout, "{}", response.to_json())?;
            stdout.flush()?;
        }
    }
    Ok(())
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
            request.id.unwrap_or(Value::Null),
            RpcError::new(INVALID_REQUEST, "invalid request"),
        )));
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(server: &mut Server, line: &str) -> Value {
        let response = server.handle_line(line).expect("debe responder");
        serde_json::from_str(&response.to_json()).unwrap()
    }

    fn initialized_server() -> Server {
        let mut server = Server::new(None, Profile::Read);
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert!(r["result"].is_object());
        server
    }

    #[test]
    fn initialize_echoes_supported_version() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "osiris");
        assert_eq!(r["result"]["capabilities"]["tools"], json!({}));
    }

    #[test]
    fn initialize_with_unsupported_version_offers_latest() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(r["result"]["protocolVersion"], LATEST_VERSION);
    }

    #[test]
    fn initialize_without_version_is_domain_validation_error() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        );
        assert_eq!(r["error"]["code"], -32000);
        assert_eq!(r["error"]["data"]["code"], "validation");
        assert_eq!(r["error"]["data"]["retryable"], false);
    }

    #[test]
    fn requests_before_initialize_are_rejected_except_ping() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"osiris_diagnose"}}"#,
        );
        assert_eq!(r["error"]["code"], NOT_INITIALIZED);
        // ping sí está permitido antes de initialize.
        let r = call(&mut server, r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
        assert_eq!(r["result"], json!({}));
        // y la puerta no se abre sola: sigue rechazando hasta initialize.
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        );
        assert_eq!(r["error"]["code"], NOT_INITIALIZED);
    }

    #[test]
    fn initialize_is_single_shot() {
        let mut server = initialized_server();
        let r = call(
            &mut server,
            r#"{"jsonrpc":"2.0","id":9,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        );
        assert_eq!(r["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let mut server = initialized_server();
        let r = call(&mut server, r#"{"jsonrpc":"2.0","id":9,"method":"nope/x"}"#);
        assert_eq!(r["error"]["code"], -32601);
        assert_eq!(r["id"], 9);
    }

    #[test]
    fn malformed_json_is_parse_error_with_null_id() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(&mut server, "esto no es json");
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn wrong_jsonrpc_version_is_invalid_request() {
        let mut server = Server::new(None, Profile::Read);
        let r = call(&mut server, r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#);
        assert_eq!(r["error"]["code"], -32600);
    }

    #[test]
    fn oversized_message_is_rejected_and_server_survives() {
        let mut server = initialized_server();
        let big = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"ping","padding":"{}"}}"#,
            "x".repeat(MAX_MESSAGE_BYTES)
        );
        let r = call(&mut server, &big);
        assert_eq!(r["error"]["code"], -32600);
        assert_eq!(r["error"]["message"], "message too large");
        // El servidor sigue operativo tras el rechazo.
        let r = call(&mut server, r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#);
        assert_eq!(r["result"], json!({}));
    }

    #[test]
    fn notifications_get_no_response() {
        let mut server = initialized_server();
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"ping"}"#)
                .is_none()
        );
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

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn handle_line_never_panics(s in ".*") {
                let mut server = Server::new(None, Profile::Read);
                let _ = server.handle_line(&s);
            }
        }
    }
}
