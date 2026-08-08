//! Tipos JSON-RPC 2.0 para el canal MCP (stdio, mensajes delimitados por líneas).
//!
//! Solo lo que el servidor necesita hoy: petición, respuesta y error. Los
//! códigos -32700/-32600/-32601 son los del estándar JSON-RPC; los errores de
//! dominio usan -32000 y exponen la categoría estable en `data.code`
//! (error-model.md §3). El mensaje externo nunca incluye la causa interna.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
/// Error de dominio; el detalle seguro viaja en `data`.
pub const DOMAIN_ERROR: i64 = -32000;

/// Petición o notificación JSON-RPC ya validada.
#[derive(Debug, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Respuesta JSON-RPC: `result` o `error`, nunca ambos.
#[derive(Debug, Serialize)]
pub struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

/// Objeto `error` de JSON-RPC.
#[derive(Debug, Serialize)]
pub struct RpcError {
    code: i64,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Mapea un error de dominio sin exponer su causa interna: el mensaje es
    /// la categoría estable y el detalle seguro viaja en `data` (FND-004).
    #[must_use]
    pub fn from_domain(e: &osiris_domain::DomainError) -> Self {
        Self {
            code: DOMAIN_ERROR,
            message: e.code().to_string(),
            data: Some(serde_json::json!({
                "code": e.code(),
                "retryable": e.retryable(),
            })),
        }
    }
}

impl Response {
    #[must_use]
    pub const fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub const fn err(id: Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Serializa la respuesta a una línea JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal"}}"#
                .to_string()
        })
    }

    /// `true` si la respuesta es un error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}
