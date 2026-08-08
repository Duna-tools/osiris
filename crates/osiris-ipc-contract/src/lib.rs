//! Contrato IPC tipado y versionado entre el núcleo y el puente Godot (ADR-004).
//!
//! Wire format: NDJSON sobre TCP loopback (una línea UTF-8 por mensaje). La
//! versión del protocolo es entera; el handshake la intercambia en `hello` /
//! `hello_ack` y el puente rechaza versiones incompatibles con `hello_error`.
//!
//! Mensajes: `hello`, `request`, `cancel` (núcleo → puente); `hello_ack`,
//! `hello_error`, `response` (puente → núcleo). Los errores usan las
//! categorías estables de `DomainError.code()` (error-model.md §3).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Versión actual del protocolo IPC. Incompatible si difiere la mayor.
pub const PROTOCOL_VERSION: u32 = 1;

/// Tamaño máximo de una línea NDJSON IPC (bytes).
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// Mensajes que envía el núcleo al puente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Primer mensaje tras conectar: versión y credencial local.
    Hello { protocol: u32, token: String },
    /// Orden tipada para el puente (`op`: p. ej. "ping", "`get_version`").
    Request {
        id: u64,
        op: String,
        #[serde(default)]
        params: Value,
    },
    /// Cancelación best-effort de una solicitud en curso.
    Cancel { id: u64 },
}

/// Mensajes que envía el puente al núcleo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BridgeMessage {
    /// Handshake aceptado; incluye la versión de Godot del editor.
    HelloAck { protocol: u32, godot: String },
    /// Handshake rechazado (`code`: "authentication" | "compatibility").
    HelloError { code: String, message: String },
    /// Resultado de una solicitud: `ok` o `error`, nunca ambos.
    Response {
        id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        ok: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<IpcError>,
    },
}

/// Error estructurado del contrato IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcError {
    /// Categoría estable (`DomainError.code()`).
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl ClientMessage {
    /// `hello` con la versión de protocolo de esta build.
    pub fn hello(token: impl Into<String>) -> Self {
        Self::Hello {
            protocol: PROTOCOL_VERSION,
            token: token.into(),
        }
    }
}

impl BridgeMessage {
    #[must_use]
    pub const fn ok(id: u64, value: Value) -> Self {
        Self::Response {
            id,
            ok: Some(value),
            error: None,
        }
    }

    #[must_use]
    pub const fn err(id: u64, error: IpcError) -> Self {
        Self::Response {
            id,
            ok: None,
            error: Some(error),
        }
    }
}

/// Serializes a message to its NDJSON line.
///
/// # Errors
/// Returns `Err` if serialization fails.
pub fn encode(msg: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string(msg)
}

/// Parses an NDJSON line to a bridge message.
///
/// # Errors
/// Returns `Err` if the line is not valid JSON or unknown type.
pub fn decode_bridge(line: &str) -> Result<BridgeMessage, serde_json::Error> {
    serde_json::from_str(line)
}

/// Parses an NDJSON line to a client message.
///
/// # Errors
/// Returns `Err` if the line is not valid JSON or unknown type.
pub fn decode_client(line: &str) -> Result<ClientMessage, serde_json::Error> {
    serde_json::from_str(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hello_carries_current_protocol_version() {
        let line = encode(&ClientMessage::hello("tok")).unwrap();
        assert_eq!(line, r#"{"type":"hello","protocol":1,"token":"tok"}"#);
        let parsed = decode_client(&line).unwrap();
        assert_eq!(parsed, ClientMessage::hello("tok"));
    }

    #[test]
    fn request_roundtrip_with_default_params() {
        let line = r#"{"type":"request","id":7,"op":"get_version"}"#;
        assert_eq!(
            decode_client(line).unwrap(),
            ClientMessage::Request {
                id: 7,
                op: "get_version".to_string(),
                params: Value::Null,
            }
        );
    }

    #[test]
    fn response_ok_and_error_shapes() {
        let ok = encode(&BridgeMessage::ok(3, json!({"godot": "4.5"}))).unwrap();
        assert_eq!(ok, r#"{"type":"response","id":3,"ok":{"godot":"4.5"}}"#);
        let err = BridgeMessage::err(
            3,
            IpcError {
                code: "validation".into(),
                message: "op desconocida".into(),
                retryable: false,
            },
        );
        let parsed = decode_bridge(&encode(&err).unwrap()).unwrap();
        assert_eq!(parsed, err);
        let BridgeMessage::Response { ok, error, .. } = parsed else {
            panic!("response");
        };
        assert!(ok.is_none() && error.is_some());
    }

    #[test]
    fn hello_error_shape() {
        let msg = BridgeMessage::HelloError {
            code: "authentication".into(),
            message: "token inválido".into(),
        };
        assert_eq!(
            encode(&msg).unwrap(),
            r#"{"type":"hello_error","code":"authentication","message":"token inválido"}"#
        );
    }

    #[test]
    fn unknown_or_malformed_lines_fail() {
        assert!(decode_bridge("no json").is_err());
        assert!(decode_bridge(r#"{"type":"misterio"}"#).is_err());
        assert!(decode_client(r#"{"type":"request","op":"ping"}"#).is_err()); // falta id
    }

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn bridge_message_roundtrip(
                _id in 0u64..1000,
                godot in "[a-z0-9.]+",
            ) {
                let msg = BridgeMessage::HelloAck { protocol: 1, godot };
                let encoded = encode(&msg).unwrap();
                let decoded = decode_bridge(&encoded).unwrap();
                assert_eq!(msg, decoded);
            }

            #[test]
            fn client_request_roundtrip(
                id in 0u64..1000,
                op in "[a-z_]+",
            ) {
                let msg = ClientMessage::Request { id, op, params: Value::Null };
                let encoded = encode(&msg).unwrap();
                let decoded = decode_client(&encoded).unwrap();
                assert_eq!(msg, decoded);
            }

            #[test]
            fn arbitrary_strings_dont_panic(s in ".*") {
                let _ = decode_bridge(&s);
                let _ = decode_client(&s);
            }
        }
    }
}
