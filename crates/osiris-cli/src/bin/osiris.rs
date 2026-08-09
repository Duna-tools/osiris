//! CLI de conformance y diagnóstico de Osiris (MVP 1, roadmap osiris-godot-mcp-docs).
//!
//! Subcomandos:
//!   osiris doctor            — verifica el protocolo MCP del servidor local (initialize, ping, tools/list).
//!   osiris conformance       — batería de checks de protocolo (initialize, ping, malformed, límites).
//!
//! El transporte es stdio (decisión D-001): el CLI lanza `osiris-server` y habla JSON-RPC.

use std::io::Write;
use std::process::{Command, Stdio};

use osiris_domain::GodotPort;

fn server_bin() -> String {
    // `osiris` y `osiris-server` son bins del mismo crate: comparten directorio.
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("osiris-server")))
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "osiris-server".to_string())
}

struct Check {
    name: &'static str,
    pass: bool,
    detail: String,
}

impl Check {
    fn new(name: &'static str, pass: bool, detail: impl Into<String>) -> Self {
        Self {
            name,
            pass,
            detail: detail.into(),
        }
    }
}

/// Lanza `osiris-server`, envía el input por stdin y devuelve las líneas JSON-RPC de stdout.
fn run_server(input: &str) -> Result<Vec<serde_json::Value>, String> {
    let bin = server_bin();
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("no se pudo lanzar {bin}: {e}"))?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .map_err(|e| format!("escritura stdin: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("esperar al servidor: {e}"))?;
    let stdout = String::from_utf8(out.stdout).map_err(|_| "stdout inválido")?;
    stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "stdout no es JSON-RPC válido".to_string())
}

fn initialize() -> &'static str {
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"osiris-conformance","version":"0.1"}}}"#
}

fn run_checks() -> Vec<Check> {
    let mut checks = Vec::new();

    let lines = match run_server(initialize()) {
        Ok(l) => l,
        Err(e) => {
            checks.push(Check::new("spawn", false, e));
            return checks;
        }
    };
    let ok = lines.first().and_then(|v| v.get("result")).is_some();
    checks.push(Check::new(
        "initialize",
        ok,
        lines
            .first()
            .map_or_else(|| "sin respuesta".to_string(), |v| v.to_string()),
    ));

    let input = [
        initialize(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#,
    ]
    .join("\n");
    match run_server(&input) {
        Ok(lines) if lines.len() == 3 => {
            checks.push(Check::new(
                "ping",
                lines[1]["result"].is_object(),
                "pong ok",
            ));
            checks.push(Check::new(
                "tools/list",
                lines[2]["result"]["tools"].is_array(),
                "catálogo ok",
            ));
        }
        Ok(lines) => checks.push(Check::new("ciclo", false, format!("{lines:?}"))),
        Err(e) => checks.push(Check::new("ciclo", false, e)),
    }

    let input = [
        initialize(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "basura",
    ]
    .join("\n");
    match run_server(&input) {
        Ok(lines) => {
            let parse_ok = lines[1]["error"]["code"] == -32700;
            checks.push(Check::new(
                "malformed-json",
                parse_ok,
                "espera -32700".to_string(),
            ));
        }
        Err(e) => checks.push(Check::new("malformed-json", false, e)),
    }

    let input = [
        initialize(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"no-existe"}}"#,
    ]
    .join("\n");
    match run_server(&input) {
        Ok(lines) => {
            let unknown = lines[1]["error"]["code"] == -32602;
            checks.push(Check::new(
                "tool-desconocida",
                unknown,
                "espera -32602".to_string(),
            ));
        }
        Err(e) => checks.push(Check::new("tool-desconocida", false, e)),
    }

    checks
}

/// Check del contrato Godot (RF-13): handshake IPC con un bridge real. Solo se
/// ejecuta si se pasan `--ipc-addr` y `--ipc-token-file`.
fn check_godot_contract(addr: &str, token_file: &str) -> Check {
    let token = match std::fs::read_to_string(token_file) {
        Ok(t) => t.trim().to_string(),
        Err(e) => return Check::new("godot-contract", false, format!("token file: {e}")),
    };
    let timeout = std::time::Duration::from_secs(3);
    match osiris_ipc_client::IpcClient::connect(addr, &token, timeout) {
        Ok(c) => Check::new(
            "godot-contract",
            true,
            format!("handshake ok, godot {}", c.godot_version()),
        ),
        Err(e) => Check::new("godot-contract", false, format!("{e}")),
    }
}

fn report(checks: &[Check]) {
    let mut passed = 0;
    for c in checks {
        let mark = if c.pass { "PASS" } else { "FAIL" };
        if c.pass {
            passed += 1;
        }
        println!("[{mark}] {:<18} {}", c.name, c.detail);
    }
    println!("resumen: {}/{} checks pasados", passed, checks.len());
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let sub = args.first().map(String::as_str).unwrap_or("doctor");
    let mut ipc_addr: Option<String> = None;
    let mut ipc_token_file: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ipc-addr" if i + 1 < args.len() => {
                ipc_addr = Some(args[i + 1].clone());
                i += 2;
            }
            "--ipc-token-file" if i + 1 < args.len() => {
                ipc_token_file = Some(args[i + 1].clone());
                i += 2;
            }
            other => {
                eprintln!("argumento desconocido: {other}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let mut checks = match sub {
        "doctor" | "conformance" => run_checks(),
        other => {
            eprintln!("subcomando desconocido: {other}");
            eprintln!("uso: osiris [doctor|conformance] [--ipc-addr ADDR] [--ipc-token-file FILE]");
            return std::process::ExitCode::from(2);
        }
    };
    if let (Some(addr), Some(token_file)) = (ipc_addr, ipc_token_file) {
        checks.push(check_godot_contract(&addr, &token_file));
    }
    report(&checks);
    if checks.iter().all(|c| c.pass) {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
