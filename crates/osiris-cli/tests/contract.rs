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

fn run_server_with_args(args: &[&str], input: &str) -> (Vec<Value>, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_osiris-server"))
        .args(args)
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
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        writeln!(
            writer,
            r#"{{"type":"hello_ack","protocol":1,"godot":"4.5.1"}}"#
        )
        .unwrap();
        writer.flush().unwrap();
        // Responde a todas las peticiones hasta que el cliente cierre.
        loop {
            line.clear();
            let n = reader.read_line(&mut line).unwrap();
            if n == 0 {
                break;
            }
            let msg: Value = serde_json::from_str(&line).unwrap();
            let id = msg["id"].as_u64().unwrap();
            if msg["op"] == "get_version" {
                writeln!(writer, r#"{{"type":"response","id":{id},"ok":{{"godot":"4.5.1","project":"test-project"}}}}"#).unwrap();
            } else if msg["op"] == "get_active_scene" {
                writeln!(writer, r#"{{"type":"response","id":{id},"ok":{{"path":"res://main.tscn","root_name":"Main","node_count":5}}}}"#).unwrap();
            } else if msg["op"] == "get_property" {
                writeln!(writer, r#"{{"type":"response","id":{id},"ok":{{"path":".","property":"position","value":{{"x":1.5,"y":2.0}}}}}}"#).unwrap();
            } else if msg["op"] == "get_editor_errors" {
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"errors":[]}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "create_node" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":"./Sprite","type":"Node2D","name":"Sprite"}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "remove_node" {
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"removed":"OldNode"}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "set_property" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"property":"position","value":{{"x":10.0,"y":20.0}}}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "call_method" {
                let method = msg["params"]["method"].as_str().unwrap_or("");
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":".","method":"{method}","result":{{"x":1,"y":2}}}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "object_property_get" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"target":".","property":"position","value":{{"x":1.5,"y":2.0}}}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "object_property_set" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"target":".","property":"position","value":{{"x":9.0,"y":9.0}}}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "save_scene" {
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"saved":"res://main.tscn"}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "run_project" {
                let scene = msg["params"]["scene"].as_str().unwrap_or("main");
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"running":true,"scene":"{scene}"}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "stop_project" || msg["op"] == "get_run_status" {
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"running":false}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "get_run_output" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"output":"linea1\\nlinea2","running":false}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "assert_property" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"match":true,"actual":100,"expected":100}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "validate_script" {
                writeln!(
                    writer,
                    r#"{{"type":"response","id":{id},"ok":{{"valid":true}}}}"#
                )
                .unwrap();
            } else if msg["op"] == "write_script" {
                let path = msg["params"]["path"].as_str().unwrap_or("res://x.gd");
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":"{path}","written":true,"chars":10}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "describe_class" {
                let klass = msg["params"]["class"].as_str().unwrap_or("Node");
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"class":"{klass}","inherits":"Node","properties":[{{"name":"position","type":"Vector2"}}],"methods":[{{"name":"queue_free","args":[]}}],"signals":["tree_entered"],"constants":[]}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "list_signals" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":".","signals":[{{"name":"tree_entered","args":[]}}]}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "connect_signal" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"connected":true,"signal":"pressed","target_path":"./Btn","method":"on_pressed"}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "disconnect_signal" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"disconnected":true,"signal":"pressed","target_path":"./Btn","method":"on_pressed"}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "asset_list" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":"res://","items":[{{"path":"res://main.tscn","type":"PackedScene","imported":true}}],"has_more":false,"scanning":false}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "project_files" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":"res://","items":[{{"path":"res://main.tscn","extension":"tscn","size":123}}],"has_more":false}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "project_settings_get" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"settings":{{"application/config/name":"My Game"}}}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "project_settings_set" {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"name":"{name}","saved":true,"value":"My Game"}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "asset_reimport" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"path":"res://icon.svg","queued":true}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "resource_assign" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"property":"texture","resource":"res://icon.svg","assigned":true}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "runtime_tree" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"tree":[{{"child_count":0,"name":"Btn","type":"Button","id":7,"scene_file":""}}],"source":"runtime"}}}}"#
            )
            .unwrap();
            } else if msg["op"] == "runtime_metrics" {
                writeln!(
                writer,
                r#"{{"type":"response","id":{id},"ok":{{"metrics":[{{"name":"time/fps","value":60.0}}],"running":true}}}}"#
            )
            .unwrap();
            } else {
                writeln!(writer, r#"{{"type":"response","id":{id},"ok":{{}}}}"#).unwrap();
            }
            writer.flush().unwrap();
        }
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
fn asset_list_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.asset.list"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["items"][0]["path"], "res://main.tscn");
    assert_eq!(r["items"][0]["type"], "PackedScene");
}
#[test]
fn project_files_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.project.files"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["items"][0]["path"], "res://main.tscn");
    assert_eq!(r["items"][0]["extension"], "tscn");
    assert_eq!(r["items"][0]["size"], 123);
}

#[test]
fn project_settings_get_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) =
        spawn_bridge_server(&addr, &token_path, &["godot.project.settings.get"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["settings"]["application/config/name"], "My Game");
}

#[test]
fn project_settings_set_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) =
        spawn_bridge_server(&addr, &token_path, &["godot.project.settings.set"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["saved"], true);
    assert_eq!(r["value"], "My Game");
}

#[test]
fn project_settings_set_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.project.settings.set","arguments":{"name":"application/config/name","value":"X"}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn asset_reimport_and_resource_assign_with_edit() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.asset.reimport"], "edit");
    assert_eq!(lines.len(), 2);
    let ri: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(ri["queued"], true);

    let (addr2, _b2, tp2) = fake_bridge("tok2");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr2, &tp2, &["godot.resource.assign"], "edit");
    assert_eq!(lines.len(), 2);
    let ra: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(ra["assigned"], true);
    assert_eq!(ra["property"], "texture");
}

#[test]
fn resource_assign_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.resource.assign","arguments":{"path":".","property":"texture","resource":"res://icon.svg"}}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn godot_editor_errors_tools_with_and_without_ipc() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.editor.errors"}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);

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
    for i in [1, 2] {
        assert_eq!(lines[i]["result"]["isError"], false);
    }
}

#[test]
fn save_scene_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _stderr) = spawn_bridge_server(&addr, &token_path, &["godot.scene.save"], "edit");
    assert_eq!(lines.len(), 2);
    let saved: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(saved["saved"], "res://main.tscn");
}

#[test]
fn save_scene_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.scene.save"}}"#,
    ].join("
");
    let (lines, _, _) = run_server(&input);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn run_stop_status_tools_with_admin_profile() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));

    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.run.status"], "read");
    assert_eq!(lines.len(), 2);
    let status: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(status["running"], false);

    let (addr2, _b2, tp2) = fake_bridge("tok2");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr2, &tp2, &["godot.run"], "admin");
    assert_eq!(lines.len(), 2);
    let run: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(run["running"], true);
}

#[test]
fn run_tool_rejected_on_edit_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.run"}}"#,
    ].join("
");
    let (lines, _, _) = run_server(&input);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn assert_property_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.assert.property"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["match"], true);
    assert_eq!(r["actual"], 100);
}

#[test]
fn validate_script_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.script.validate"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["valid"], true);
}

#[test]
fn write_script_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.script.write"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["written"], true);
    assert_eq!(r["path"], "res://x.gd");
}

#[test]
fn write_script_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.script.write","arguments":{"path":"res://x.gd","code":"extends Node2D"}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn describe_class_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.describe","arguments":{"class":"Node2D"}}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server_with_args(
        &[
            "--ipc-addr",
            &addr,
            "--ipc-token-file",
            &token_path,
            "--capability-profile",
            "read",
        ],
        &input,
    );
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["class"], "Node2D");
    assert_eq!(r["inherits"], "Node");
    assert_eq!(r["properties"][0]["name"], "position");
    assert_eq!(r["signals"][0], "tree_entered");
}

#[test]
fn describe_class_rejected_without_class() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.describe"}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "ipc");
}

#[test]
fn signal_list_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.signal.list"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["signals"][0]["name"], "tree_entered");
}

#[test]
fn object_call_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["object.call"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["result"]["x"], 1);
    assert_eq!(r["result"]["y"], 2);
}

#[test]
fn object_call_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"object.call","arguments":{"path":".","method":"queue_free"}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn object_property_get_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["object.property.get"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["property"], "position");
    assert_eq!(r["value"]["x"], 1.5);
}

#[test]
fn object_property_set_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["object.property.set"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["value"]["x"], 9.0);
}

#[test]
fn signal_connect_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.signal.connect"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["connected"], true);
    assert_eq!(r["signal"], "pressed");
    assert_eq!(r["method"], "on_pressed");
}

#[test]
fn signal_connect_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.signal.connect","arguments":{"path":".","signal":"pressed","target_path":"./Btn","method":"on_pressed"}}}"#,
    ].join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn signal_disconnect_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.signal.disconnect"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["disconnected"], true);
}

#[test]
fn run_project_with_custom_scene_and_timeout() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"godot.run","arguments":{"scene":"res://levels/test.tscn","timeout_secs":5}}}"#,
    ].join("\n");

    let (lines, _, ok) = run_server_with_args(
        &[
            "--ipc-addr",
            &addr,
            "--ipc-token-file",
            &token_path,
            "--capability-profile",
            "admin",
        ],
        &input,
    );
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["running"], true);
    assert_eq!(r["scene"], "res://levels/test.tscn");
}

#[test]
fn health_reports_status() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"health"}"#,
    ].join("
");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let h = &lines[1]["result"];
    assert_eq!(h["ok"], true);
    assert_eq!(h["version"], "0.1.0");
    assert_eq!(h["profile"], "read");
    assert_eq!(h["godot_connected"], false);
}

#[test]
fn run_output_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.run.output"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(r["output"].as_str().unwrap().contains("linea1"));
    assert_eq!(r["running"], false);
}

#[test]
fn audit_log_emits_required_fields() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cliente-test","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"osiris_diagnose"}}"#,
    ]
    .join("\n");
    let (lines, stderr, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let plain = strip_ansi(&stderr);
    let audit = plain.lines().find(|l| l.contains("osiris_audit"));
    assert!(
        audit.is_some(),
        "debe haber un evento de auditoría: {plain}"
    );
    let line = audit.unwrap();
    assert!(
        line.contains("actor=Some(\"cliente-test\")"),
        "actor presente: {line}"
    );
    assert!(
        line.contains("tool=\"osiris_diagnose\""),
        "tool presente: {line}"
    );
    assert!(line.contains("params_sha="), "params sanitizados: {line}");
    assert!(line.contains("effect=\"read\""), "efecto presente: {line}");
    assert!(line.contains("policy="), "política presente: {line}");
    assert!(line.contains("duracion_us="), "duración presente: {line}");
    assert!(
        line.contains("resultado=\"ok\""),
        "resultado presente: {line}"
    );
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            while let Some(&next) = chars.peek() {
                chars.next();
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[test]
fn plan_validate_rejects_unknown_tool() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"plan.validate","arguments":{"plan":{"steps":[{"tool":"no.existe","params":{}}]}}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "validation");
}

#[test]
fn plan_validate_ok_with_known_tools() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"plan.validate","arguments":{"plan":{"steps":[{"tool":"godot.scene.active","id":"a"}]}}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["valid"], true);
    assert_eq!(r["steps"], 1);
}

#[test]
fn plan_execute_runs_steps_and_resolves_refs() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"plan.execute","arguments":{"plan":{"steps":[{"tool":"godot_diagnose","id":"diag"},{"tool":"godot.scene.active","id":"scene","params":{"path":"$diag.godot_version"}}]}}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server_with_args(
        &[
            "--ipc-addr",
            &addr,
            "--ipc-token-file",
            &token_path,
            "--capability-profile",
            "edit",
        ],
        &input,
    );
    assert!(ok);
    assert_eq!(lines.len(), 2, "initialize + plan.execute: {lines:?}");
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["receipts"].as_array().unwrap().len(), 2);
    assert_eq!(r["receipts"][0]["id"], "diag");
    assert_eq!(r["receipts"][1]["id"], "scene");
}

#[test]
fn plan_execute_rejected_on_read_profile() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"plan.execute","arguments":{"plan":{"steps":[{"tool":"godot.scene.set_property","params":{}}]}}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "authorization");
}

#[test]
fn skill_list_lists_available_skills() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skill.list"}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let skills = r["skills"].as_array().unwrap();
    assert!(skills.iter().any(|s| s["name"] == "setup_2d_player"));
}

#[test]
fn skill_get_returns_uses_and_knowledge() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skill.get","arguments":{"name":"setup_2d_player"}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["name"], "setup_2d_player");
    assert!(
        r["uses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u == "godot.describe")
    );
    assert!(!r["knowledge"].as_array().unwrap().is_empty());
}

#[test]
fn skill_get_unknown_skill_errors() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skill.get","arguments":{"name":"nope"}}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["error"]["code"], -32000);
    assert_eq!(lines[1]["error"]["data"]["code"], "validation");
}

#[test]
fn capabilities_catalog_reflects_profile() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["osiris.capabilities"], "edit");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["profile"], "edit");
    assert_eq!(r["godot_connected"], true);
    let caps = r["capabilities"].as_array().unwrap();
    assert!(caps.iter().any(|c| c == "GodotSceneWrite"));
    assert!(!caps.iter().any(|c| c == "GodotRun"));
    let tools = r["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "godot.scene.create_node"));
    assert!(tools.iter().any(|t| t["name"] == "skill.get"));
}

#[test]
fn capabilities_read_profile_excludes_write_tools() {
    let input = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"osiris.capabilities"}}"#,
    ]
    .join("\n");
    let (lines, _, ok) = run_server(&input);
    assert!(ok);
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["profile"], "read");
    assert_eq!(r["godot_connected"], false);
    let tools = r["tools"].as_array().unwrap();
    assert!(!tools.iter().any(|t| t["name"] == "godot.scene.create_node"));
    assert!(tools.iter().any(|t| t["name"] == "godot.scene.active"));
}

#[test]
fn runtime_tree_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.runtime.tree"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["source"], "runtime");
    assert_eq!(r["tree"][0]["name"], "Btn");
    assert_eq!(r["tree"][0]["type"], "Button");
}

#[test]
fn runtime_metrics_over_ipc() {
    let (addr, _bridge, token_path) = fake_bridge("tok");
    thread::sleep(Duration::from_millis(100));
    let (lines, _) = spawn_bridge_server(&addr, &token_path, &["godot.runtime.metrics"], "read");
    assert_eq!(lines.len(), 2);
    let r: Value =
        serde_json::from_str(lines[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(r["metrics"][0]["name"], "time/fps");
    assert_eq!(r["metrics"][0]["value"], 60.0);
}
