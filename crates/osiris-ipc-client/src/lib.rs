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
    /// Configuración retenida para reconexión (UC-050).
    addr: String,
    token: String,
    timeout: Duration,
}

/// Operaciones mutables: jamás se reintentan automáticamente (UC-050).
const MUTATING_OPS: &[&str] = &[
    "create_node",
    "remove_node",
    "set_property",
    "save_scene",
    "run_project",
    "stop_project",
];

impl std::fmt::Debug for IpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcClient")
            .field("godot_version", &self.godot_version)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl osiris_domain::GodotPort for IpcClient {
    fn call(&mut self, op: &str, params: Value) -> Result<Value, DomainError> {
        IpcClient::call(self, op, params)
    }

    fn godot_version(&self) -> &str {
        &self.godot_version
    }
}

impl IpcClient {
    /// Conecta al puente, envía `hello` y valida el ack.
    ///
    /// # Errors
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
            addr: addr.to_string(),
            token: token.to_string(),
            timeout,
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
            BridgeMessage::Response { .. } => Err(DomainError::Ipc(
                "respuesta inesperada del puente durante el handshake".to_string(),
            )),
        }
    }

    /// Envía una orden y espera su respuesta (por id). Otras respuestas se
    /// descartan (no hay multiplexación hoy: una solicitud en vuelo).
    ///
    /// Si la operación es de **lectura** y el canal IPC se rompe, reconecta una
    /// vez y reintenta (UC-050). Las operaciones mutables jamás se reintentan
    /// automáticamente: devuelven el error para que el llamador decida.
    ///
    /// # Errors
    /// `Timeout`, `Ipc` o el `DomainError` que reporte el puente.
    pub fn call(&mut self, op: &str, params: Value) -> Result<Value, DomainError> {
        let span = tracing::debug_span!("ipc_call", op, fase = "ipc");
        let _enter = span.enter();
        match self.call_once(op, params.clone()) {
            Err(DomainError::Ipc(_)) if !MUTATING_OPS.contains(&op) => {
                tracing::warn!(op, "canal IPC roto en lectura; reconectando");
                self.reconnect()?;
                self.call_once(op, params)
            }
            result => result,
        }
    }

    fn call_once(&mut self, op: &str, params: Value) -> Result<Value, DomainError> {
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
                _ => {}
            }
        }
    }

    /// Reconecta al mismo puente y renegocia versión y token (UC-050).
    fn reconnect(&mut self) -> Result<(), DomainError> {
        let fresh = Self::connect(&self.addr, &self.token, self.timeout)?;
        *self = fresh;
        Ok(())
    }

    /// Cancelación best-effort (el puente puede ignorarla si ya terminó).
    ///
    /// # Errors
    /// Devuelve `DomainError::Ipc` o `DomainError::Internal` si la escritura al puente falla.
    pub fn cancel(&mut self, id: u64) -> Result<(), DomainError> {
        self.send(&ClientMessage::Cancel { id })
    }

    fn send(&mut self, msg: &ClientMessage) -> Result<(), DomainError> {
        let line = osiris_ipc_contract::encode(msg)
            .map_err(|e| DomainError::Internal(format!("codificar IPC: {e}")))?;
        writeln!(self.writer, "{line}")
            .and_then(|()| self.writer.flush())
            .map_err(|e| map_io(&e))
    }

    fn recv(&mut self) -> Result<BridgeMessage, DomainError> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).map_err(|e| map_io(&e))?;
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
fn map_io(e: &std::io::Error) -> DomainError {
    let msg = e.to_string();
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => DomainError::Timeout(msg),
        _ => DomainError::Ipc(msg),
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
            let (protocol, token) = match read_line(&mut reader) {
                ClientMessage::Hello { protocol, token } => (protocol, token),
                msg => panic!("esperaba hello, recibido: {msg:?}"),
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
            let (id, op) = match read_line(&mut reader) {
                ClientMessage::Request { id, op, .. } => (id, op),
                msg => panic!("esperaba request, recibido: {msg:?}"),
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
            let _ = read_line(&mut reader);
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
            let id = match read_line(&mut reader) {
                ClientMessage::Request { id, .. } => id,
                msg => panic!("esperaba request, recibido: {msg:?}"),
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

    #[test]
    fn read_call_reconnects_after_bridge_restart() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let accepts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&accepts);
        thread::spawn(move || {
            let (s1, _) = listener.accept().unwrap();
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut r1 = BufReader::new(s1.try_clone().unwrap());
            let mut line = String::new();
            r1.read_line(&mut line).unwrap();
            send(
                &mut s1.try_clone().unwrap(),
                &BridgeMessage::HelloAck {
                    protocol: PROTOCOL_VERSION,
                    godot: "4.5.1".into(),
                },
            );
            let mut line = String::new();
            r1.read_line(&mut line).unwrap();
            let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
            let id = msg["id"].as_u64().unwrap();
            send(
                &mut s1.try_clone().unwrap(),
                &BridgeMessage::err(
                    id,
                    IpcError {
                        code: "ipc".into(),
                        message: "canal roto".into(),
                        retryable: true,
                    },
                ),
            );
            let (s2, _) = listener.accept().unwrap();
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut r2 = BufReader::new(s2.try_clone().unwrap());
            let mut line = String::new();
            r2.read_line(&mut line).unwrap();
            send(
                &mut s2.try_clone().unwrap(),
                &BridgeMessage::HelloAck {
                    protocol: PROTOCOL_VERSION,
                    godot: "4.5.1".into(),
                },
            );
            let mut line = String::new();
            r2.read_line(&mut line).unwrap();
            let msg: serde_json::Value = serde_json::from_str(&line).unwrap();
            let id = msg["id"].as_u64().unwrap();
            send(
                &mut s2.try_clone().unwrap(),
                &BridgeMessage::ok(id, serde_json::json!({"ok": true})),
            );
        });

        let mut client = IpcClient::connect(&addr, "tok", Duration::from_secs(2)).unwrap();
        let v = client.call("get_version", Value::Null).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(accepts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn mutating_call_does_not_retry_on_broken_channel() {
        let addr = fake_bridge(|mut reader, mut stream| {
            let _ = read_line(&mut reader);
            send(
                &mut stream,
                &BridgeMessage::HelloAck {
                    protocol: PROTOCOL_VERSION,
                    godot: "4.5.1".into(),
                },
            );
            let _ = read_line(&mut reader);
        });
        let mut client = IpcClient::connect(&addr, "tok", Duration::from_secs(2)).unwrap();
        let err = client.call("set_property", Value::Null).unwrap_err();
        assert!(matches!(err, DomainError::Ipc(_) | DomainError::Timeout(_)));
    }
}
