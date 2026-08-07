//! Suite de contrato MCP mínima (T-027, test-strategy.md): ejercita el binario
//! real `osiris-server` por stdio, como lo haría un cliente MCP.
//!
//! Cubre: ciclo de vida, handshake, framing por líneas, pureza de stdout
//! (solo JSON-RPC), logs a stderr, límites de tamaño y herramientas.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;

use serde_json::{Value, json};

/// Ejecuta el binario con `input` por stdin y devuelve (líneas stdout, stderr, éxito).
fn run_server(input: &str) -> (Vec<Value>, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_osiris-server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("lanzar osiris-server");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .expect("escribir stdin"); // al soltarlo el servidor recibe EOF y termina
    let out = child.wait_with_output().expect("esperar al servidor");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines = stdout
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l)
                .unwrap_or_else(|_| panic!("stdout debe ser solo JSON-RPC, línea: {l}"))
        })
        .collect();
    (
        lines,
        String::from_utf8(out.stderr).unwrap(),
        out.status.success(),
    )
}

#[test]
fn full_lifecycle_over_stdio() {
    let input = [
        // 0: herramienta antes de initialize → -32002
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"osiris_diagnose"}}"#,
        // 1: handshake
        r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"contract","version":"0"}}}"#,
        // notificación: no debe producir línea de salida
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        // 2: tools/list declara la herramienta con esquema
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        // 3: tools/call ejecuta el diagnóstico
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"osiris_diagnose","arguments":{}}}"#,
        // 4: herramienta desconocida → -32602
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#,
        // 5: método desconocido → -32601
        r#"{"jsonrpc":"2.0","id":6,"method":"nope/x"}"#,
        // 6: JSON basura → -32700 con id null
        "basura",
        // 7: segundo initialize → -32600
        r#"{"jsonrpc":"2.0","id":8,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        // 8: ping sigue vivo al final
        r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#,
    ]
    .join("\n");

    let (lines, stderr, ok) = run_server(&input);

    assert!(ok, "el servidor debe salir limpio al EOF");
    assert_eq!(lines.len(), 9, "una línea por petición con id");

    assert_eq!(lines[0]["error"]["code"], -32002);
    assert_eq!(lines[1]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(lines[1]["result"]["serverInfo"]["name"], "osiris");
    assert!(
        lines[2]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "osiris_diagnose")
    );
    assert_eq!(lines[3]["result"]["isError"], false);
    assert!(
        lines[3]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("\"osiris\"")
    );
    assert_eq!(lines[4]["error"]["code"], -32602);
    assert_eq!(lines[5]["error"]["code"], -32601);
    assert_eq!(lines[6]["error"]["code"], -32700);
    assert_eq!(lines[6]["id"], Value::Null);
    assert_eq!(lines[7]["error"]["code"], -32600);
    assert_eq!(lines[8]["result"], json!({}));

    for line in &lines {
        assert_eq!(line["jsonrpc"], "2.0");
    }
    assert!(
        stderr.contains("servidor MCP stdio iniciado"),
        "los logs deben ir a stderr: {stderr}"
    );
}

#[test]
fn oversized_message_is_rejected() {
    let big = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"padding\":\"{}\"}}\n",
        "x".repeat(4 * 1024 * 1024)
    );
    let (lines, _, _) = run_server(&big);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["error"]["code"], -32600);
}

#[test]
fn unsupported_protocol_version_gets_latest_offer() {
    let (lines, _, _) = run_server(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
    );
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
}

fn fake_bridge(token: &'static str) -> (String, thread::JoinHandle<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let tmpdir = std::env::temp_dir().join(format!("osiris-{}", std::process::id()));
    std::fs::create_dir_all(&tmpdir).unwrap();
    let token_file = tmpdir.join("token");
    std::fs::write(&token_file, token).unwrap();

    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        // handshake
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let msg: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(msg["type"], "hello");
        assert_eq!(msg["token"], token);
        assert_eq!(msg["protocol"], 1);
        writeln!(
            writer,
            r#"{{"type":"hello_ack","protocol":1,"godot":"4.5.1"}}"#
        )
        .unwrap();
        writer.flush().unwrap();
        // process one request
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let msg: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(msg["type"], "request");
        let id = msg["id"].as_u64().unwrap();
        writeln!(
            writer,
            r#"{{"type":"response","id":{},"ok":{{"godot":"4.5.1","project":"test-project"}}}}"#,
            id
        )
        .unwrap();
        writer.flush().unwrap();
    });
    (addr, handle, token_file.to_string_lossy().to_string())
}

#[test]
fn godot_diagnose_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("test-tok");

    let mut child = Command::new(env!("CARGO_BIN_EXE_osiris-server"))
        .arg("--ipc-addr")
        .arg(&addr)
        .arg("--ipc-token-file")
        .arg(&token_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"godot_diagnose","arguments":{{}}}}}}"#
    )
    .unwrap();
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        lines.len(),
        2,
        "initialize + godot_diagnose (notification no responde)"
    );

    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    let diag = &lines[1]["result"];
    assert_eq!(diag["isError"], false);
    let payload: Value =
        serde_json::from_str(diag["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["godot_version"], "4.5.1");
    assert!(payload["godot_details"]["project"] == "test-project");
}
