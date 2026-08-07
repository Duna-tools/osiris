//! Cliente IPC: conexión TCP loopback al puente Godot, handshake de versión y
//! token (ADR-004), solicitud/respuesta por id con timeout.
//!
//! Sync a propósito: el servidor MCP stdio procesa un mensaje a la vez, así
//! que una conexión bloqueante con timeouts es suficiente hoy. La reconexión
//! automática (UC-050) se implementará donde se consuma (application), con la
//! política de reintento por tipo de operación (error-model.md §2: reintentar
//! solo lecturas seguras).

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use osiris_domain::DomainError;
use osiris_ipc_contract::{BridgeMessage, ClientMessage, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use serde_json::Value;

/// Cliente conectado y autenticado contra un puente Godot.
pub struct IpcClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    next_id: u64,
    /// Versión de Godot reportada por el puente en el handshake.
    pub godot_version: String,
}

impl std::fmt::Debug for IpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcClient")
            .field("godot_version", &self.godot_version)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl IpcClient {
    /// Conecta al puente, envía `hello` y valida el ack.
    ///
    /// # Errores
    /// - `Ipc`: dirección inválida, conexión rechazada o canal roto.
    /// - `Timeout`: el puente no respondió dentro de `timeout`.
    /// - `Authentication`: token rechazado.
    /// - `Compatibility`: versión de protocolo incompatible.
    pub fn connect(addr: &str, token: &str, timeout: Duration) -> Result<Self, DomainError> {
        let socket = addr
            .to_socket_addrs()
            .map_err(|e| DomainError::Ipc(format!("dirección inválida '{addr}': {e}")))?
            .next()
            .ok_or_else(|| DomainError::Ipc(format!("'{addr}' no resuelve a ninguna IP")))?;
        let stream = TcpStream::connect_timeout(&socket, timeout)
            .map_err(|e| DomainError::Ipc(format!("no se pudo conectar a {addr}: {e}")))?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|()| stream.set_write_timeout(Some(timeout)))
            .and_then(|()| stream.set_nodelay(true))
            .map_err(|e| DomainError::Ipc(format!("no se pudo configurar el socket: {e}")))?;

        let mut client = Self {
            writer: stream
                .try_clone()
                .map_err(|e| DomainError::Ipc(format!("try_clone: {e}")))?,
            reader: BufReader::new(stream),
            next_id: 1,
            godot_version: String::new(),
        };
        client.send(&ClientMessage::hello(token))?;
        match client.recv()? {
            BridgeMessage::HelloAck { protocol, godot } if protocol == PROTOCOL_VERSION => {
                client.godot_version = godot;
                Ok(client)
            }
            BridgeMessage::HelloAck { protocol, .. } => Err(DomainError::Compatibility(format!(
                "puente con protocolo {protocol}, este núcleo habla {PROTOCOL_VERSION}"
            ))),
            BridgeMessage::HelloError { code, message } => Err(map_code(&code, &message)),
            other => Err(DomainError::Ipc(format!(
                "handshake inesperado del puente: {other:?}"
            ))),
        }
    }

    /// Envía una orden y espera su respuesta (por id). Otras respuestas se
    /// descartan (no hay multiplexación hoy: una solicitud en vuelo).
    ///
    /// # Errores
    /// `Timeout`, `Ipc` o el `DomainError` que reporte el puente.
    pub fn call(&mut self, op: &str, params: Value) -> Result<Value, DomainError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&ClientMessage::Request {
            id,
            op: op.to_string(),
            params,
        })?;
        loop {
            match self.recv()? {
                BridgeMessage::Response { id: rid, ok, error } if rid == id => {
                    if let Some(e) = error {
                        return Err(map_code(&e.code, &e.message));
                    }
                    return Ok(ok.unwrap_or(Value::Null));
                }
                _ => continue, // respuesta de otra id o mensaje fuera de lugar
            }
        }
    }

    /// Cancelación best-effort (el puente puede no atenderla si ya terminó).
    pub fn cancel(&mut self, id: u64) -> Result<(), DomainError> {
        self.send(&ClientMessage::Cancel { id })
    }

    fn send(&mut self, msg: &ClientMessage) -> Result<(), DomainError> {
        let line = osiris_ipc_contract::encode(msg)
            .map_err(|e| DomainError::Internal(format!("codificar IPC: {e}")))?;
        writeln!(self.writer, "{line}")
            .and_then(|()| self.writer.flush())
            .map_err(map_io)
    }

    fn recv(&mut self) -> Result<BridgeMessage, DomainError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).map_err(map_io)?;
        if n == 0 {
            return Err(DomainError::Ipc("el puente cerró la conexión".to_string()));
        }
        if line.len() > MAX_MESSAGE_BYTES {
            return Err(DomainError::Ipc(
                "mensaje del puente excede el tope".to_string(),
            ));
        }
        osiris_ipc_contract::decode_bridge(&line)
            .map_err(|e| DomainError::Ipc(format!("mensaje del puente inválido: {e}")))
    }
}

/// Mapea errores de E/S: los timeouts del socket llegan como WouldBlock/TimedOut.
fn map_io(e: std::io::Error) -> DomainError {
    use std::io::ErrorKind::*;
    match e.kind() {
        WouldBlock | TimedOut => DomainError::Timeout(e.to_string()),
        _ => DomainError::Ipc(e.to_string()),
    }
}

/// Mapea el código estable del contrato a su variante de dominio.
fn map_code(code: &str, message: &str) -> DomainError {
    let msg = message.to_string();
    match code {
        "validation" => DomainError::Validation(msg),
        "authentication" => DomainError::Authentication,
        "authorization" => DomainError::Authorization(msg),
        "compatibility" => DomainError::Compatibility(msg),
        "saturation" => DomainError::Saturation(msg),
        "timeout" => DomainError::Timeout(msg),
        "godot" => DomainError::Godot(msg),
        "conflict" => DomainError::Conflict(msg),
        "internal" => DomainError::Internal(msg),
        _ => DomainError::Ipc(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use osiris_ipc_contract::IpcError;
    use std::net::TcpListener;
    use std::thread;

    /// Puente falso: acepta una conexión y ejecuta `script` con ella.
    fn fake_bridge(
        script: impl FnOnce(BufReader<TcpStream>, TcpStream) + Send + 'static,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader = BufReader::new(stream.try_clone().unwrap());
            script(reader, stream);
        });
        addr
    }

    fn read_line(reader: &mut BufReader<TcpStream>) -> ClientMessage {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        osiris_ipc_contract::decode_client(&line).unwrap()
    }

    fn send(stream: &mut TcpStream, msg: &BridgeMessage) {
        let line = osiris_ipc_contract::encode(msg).unwrap();
        writeln!(stream, "{line}").unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn handshake_and_call_roundtrip() {
        let addr = fake_bridge(|mut reader, mut stream| {
            let ClientMessage::Hello { protocol, token } = read_line(&mut reader) else {
                panic!("esperaba hello");
            };
            assert_eq!(protocol, PROTOCOL_VERSION);
            assert_eq!(token, "secreto");
            send(
                &mut stream,
                &BridgeMessage::HelloAck {
                    protocol: PROTOCOL_VERSION,
                    godot: "4.5.1".into(),
                },
            );
            let ClientMessage::Request { id, op, .. } = read_line(&mut reader) else {
                panic!("esperaba request");
            };
            assert_eq!(op, "get_version");
            send(
                &mut stream,
                &BridgeMessage::ok(id, serde_json::json!({"godot": "4.5.1"})),
            );
        });

        let mut client = IpcClient::connect(&addr, "secreto", Duration::from_secs(2)).unwrap();
        assert_eq!(client.godot_version, "4.5.1");
        let v = client.call("get_version", Value::Null).unwrap();
        assert_eq!(v["godot"], "4.5.1");
    }

    #[test]
    fn wrong_token_is_authentication_error() {
        let addr = fake_bridge(|mut reader, mut stream| {
            let _ = read_line(&mut reader);
            send(
                &mut stream,
                &BridgeMessage::HelloError {
                    code: "authentication".into(),
                    message: "token inválido".into(),
                },
            );
        });
        let err = IpcClient::connect(&addr, "malo", Duration::from_secs(2)).unwrap_err();
        assert!(matches!(err, DomainError::Authentication));
        assert!(!err.retryable());
    }

    #[test]
    fn incompatible_protocol_is_compatibility_error() {
        let addr = fake_bridge(|mut reader, mut stream| {
            let _ = read_line(&mut reader);
            send(
                &mut stream,
                &BridgeMessage::HelloAck {
                    protocol: 999,
                    godot: "9.9".into(),
                },
            );
        });
        let err = IpcClient::connect(&addr, "tok", Duration::from_secs(2)).unwrap_err();
        assert!(matches!(err, DomainError::Compatibility(_)));
    }

    #[test]
    fn silent_bridge_times_out() {
        let addr = fake_bridge(|mut reader, _stream| {
            let _ = read_line(&mut reader); // acepta pero no responde jamás
            thread::sleep(Duration::from_secs(5));
        });
        let err = IpcClient::connect(&addr, "tok", Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err, DomainError::Timeout(_)));
    }

    #[test]
    fn bridge_error_maps_to_domain_variant() {
        let addr = fake_bridge(|mut reader, mut stream| {
            let _ = read_line(&mut reader);
            send(
                &mut stream,
                &BridgeMessage::HelloAck {
                    protocol: PROTOCOL_VERSION,
                    godot: "4.5.1".into(),
                },
            );
            let ClientMessage::Request { id, .. } = read_line(&mut reader) else {
                panic!("esperaba request");
            };
            send(
                &mut stream,
                &BridgeMessage::err(
                    id,
                    IpcError {
                        code: "godot".into(),
                        message: "no hay escena activa".into(),
                        retryable: false,
                    },
                ),
            );
        });
        let mut client = IpcClient::connect(&addr, "tok", Duration::from_secs(2)).unwrap();
        let err = client.call("get_open_scene", Value::Null).unwrap_err();
        assert!(matches!(err, DomainError::Godot(_)));
        assert_eq!(err.code(), "godot");
    }
}
