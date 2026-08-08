//! Suite de contrato MCP (T-027, test-strategy.md): ejercita el binario real
//! `osiris-server` por stdio.
//!
//! Cubre: ciclo de vida, handshake, pureza de stdout, límites, herramientas y
//! errores esperados sin puente Godot.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

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
        .expect("escribir stdin");
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
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"osiris_diagnose"}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"contract","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"osiris_diagnose","arguments":{}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#,
        r#"{"jsonrpc":"2.0","id":6,"method":"nope/x"}"#,
        "basura",
        r#"{"jsonrpc":"2.0","id":8,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#,
    ]
    .join("\n");

    let (lines, stderr, ok) = run_server(&input);
    assert!(ok, "el servidor debe salir limpio al EOF");
    assert_eq!(lines.len(), 9, "stderr: {stderr}");

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

#[test]
fn godot_scene_tools_error_without_ipc() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.scene.active"}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"godot.scene.nodes","arguments":{"depth":2}}}"#,
    ]
    .join("\n");

    let (lines, _stderr, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 3, "3 líneas: initialize + 2 tools/call");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    for i in [1, 2] {
        assert_eq!(
            lines[i]["error"]["code"], -32000,
            "error de dominio en línea {i}"
        );
        assert_eq!(lines[i]["error"]["data"]["code"], "ipc");
    }
}

fn fake_bridge(token: &str) -> (String, thread::JoinHandle<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let tmpdir = std::env::temp_dir().join(format!("osiris-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&tmpdir).unwrap();
    let token_file = tmpdir.join("token");
    let tok = token.to_string();
    std::fs::write(&token_file, &tok).unwrap();
    let token_path = token_file.to_string_lossy().to_string();

    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut writer = stream;
        // handshake
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        writeln!(
            writer,
            r#"{{"type":"hello_ack","protocol":1,"godot":"4.5.1"}}"#
        )
        .unwrap();
        writer.flush().unwrap();
        // first request
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let msg: Value = serde_json::from_str(&line).unwrap();
        let id = msg["id"].as_u64().unwrap();
        if msg["op"] == "get_version" {
            writeln!(writer, r#"{{"type":"response","id":{},"ok":{{"godot":"4.5.1","project":"test-project"}}}}"#, id).unwrap();
        } else if msg["op"] == "get_active_scene" {
            writeln!(writer, r#"{{"type":"response","id":{},"ok":{{"path":"res://main.tscn","root_name":"Main","node_count":5}}}}"#, id).unwrap();
        } else if msg["op"] == "get_property" {
            writeln!(writer, r#"{{"type":"response","id":{},"ok":{{"path":".","property":"position","value":{{"x":1.5,"y":2.0}}}}}}"#, id).unwrap();
        } else if msg["op"] == "get_editor_errors" {
            writeln!(
                writer,
                r#"{{"type":"response","id":{},"ok":{{"errors":[]}}}}"#,
                id
            )
            .unwrap();
        } else if msg["op"] == "create_node" {
            writeln!(
                writer,
                r#"{{"type":"response","id":{},"ok":{{"path":"./Sprite","type":"Node2D","name":"Sprite"}}}}"#,
                id
            )
            .unwrap();
        } else if msg["op"] == "remove_node" {
            writeln!(
                writer,
                r#"{{"type":"response","id":{},"ok":{{"removed":"OldNode"}}}}"#,
                id
            )
            .unwrap();
        } else if msg["op"] == "set_property" {
            writeln!(
                writer,
                r#"{{"type":"response","id":{},"ok":{{"property":"position","value":{{"x":10.0,"y":20.0}}}}}}"#,
                id
            )
            .unwrap();
        } else {
            writeln!(writer, r#"{{"type":"response","id":{},"ok":{{}}}}"#, id).unwrap();
        }
        writer.flush().unwrap();
    });
    (addr, handle, token_path)
}

fn spawn_bridge_server(
    addr: &str,
    token_path: &str,
    tools: &[&str],
    profile: &str,
) -> (Vec<Value>, String) {
    let input = tools
        .iter()
        .enumerate()
        .flat_map(|(i, t)| {
            if i == 0 {
                vec![
                    format!(
                        r#"{{"jsonrpc":"2.0","id":{},"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}"#,
                        i
                    ),
                    r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_string(),
                ]
            } else {
                vec![]
            }
            .into_iter()
            .chain(std::iter::once(format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{}","arguments":{{}}}}}}"#,
                i + 100,
                t
            )))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut child = Command::new(env!("CARGO_BIN_EXE_osiris-server"))
        .arg("--ipc-addr")
        .arg(addr)
        .arg("--ipc-token-file")
        .arg(token_path)
        .arg("--capability-profile")
        .arg(profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let stderr = String::from_utf8(out.stderr).unwrap();
    (lines, stderr)
}

#[test]
fn godot_diagnose_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    // Small sleep to let the bridge thread start listening
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) = spawn_bridge_server(&addr, &token_path, &["godot_diagnose"], "read");
    assert_eq!(lines.len(), 2, "initialize + godot_diagnose");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    let diag = &lines[1]["result"];
    assert_eq!(diag["isError"], false);
    let payload: Value =
        serde_json::from_str(diag["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["godot_version"], "4.5.1");
}

#[test]
fn write_tool_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.scene.create_node","arguments":{"path":".","type":"Node2D","name":"Test"}}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn godot_scene_active_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) = spawn_bridge_server(&addr, &token_path, &["godot.scene.active"], "read");
    assert_eq!(lines.len(), 2, "initialize + godot.scene.active: {lines:?}");
    let active: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(active["path"], "res://main.tscn");
    assert_eq!(active["root_name"], "Main");
    assert_eq!(active["node_count"], 5);
}

#[test]
fn godot_props_tools_error_without_ipc() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.scene.properties","arguments":{"path":".","property":"position"}}}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"godot.scene.resources"}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    for i in [1, 2] {
        assert_eq!(lines[i]["error"]["code"], -32000);
        assert_eq!(lines[i]["error"]["data"]["code"], "ipc");
    }
}

#[test]
fn godot_property_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) =
        spawn_bridge_server(&addr, &token_path, &["godot.scene.properties"], "read");
    assert_eq!(lines.len(), 2);
    let val: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(val["property"], "position");
    assert_eq!(val["value"]["x"], 1.5);
    assert_eq!(val["value"]["y"], 2.0);
}

#[test]
fn godot_editor_errors_tools_with_and_without_ipc() {
    // sin IPC: error
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.editor.errors"}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);

    // con IPC: resultado
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.editor.errors"], "read");
    assert_eq!(lines.len(), 2);
    let val: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(val["errors"].as_array().unwrap().len(), 0);
}

#[test]
fn write_tools_work_with_edit_profile() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) =
        spawn_bridge_server(&addr, &token_path, &["godot.scene.create_node"], "edit");
    assert_eq!(lines.len(), 2, "initialize + create_node: {lines:?}");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    let create: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(create["path"], "./Sprite");
    assert_eq!(create["type"], "Node2D");
}

#[test]
fn cache_reuses_read_results() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) = spawn_bridge_server(
        &addr,
        &token_path,
        &["godot_diagnose", "godot_diagnose"],
        "read",
    );
    assert_eq!(lines.len(), 3, "initialize + 2 godot_diagnose: {lines:?}");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    // Ambas respuestas deben ser OK (la segunda desde cache, sin IPC)
    for i in [1, 2] {
        assert_eq!(lines[i]["result"]["isError"], false);
    }
}
