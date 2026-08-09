//! CLI: binario `osiris-server` (deployment-model.md).
//!
//! Servidor MCP sobre stdio con puente IPC opcional a Godot.
//! Arranque: parsear args, resolver configuración, telemetría, conectar puente,
//! iniciar bucle MCP.

mod config;

use osiris_telemetry::{error, info, warn};
use std::process::ExitCode;

fn connect_ipc(cfg: &config::Config) -> Option<osiris_ipc_client::IpcClient> {
    let addr = cfg.ipc_addr.as_ref()?;
    let token = cfg
        .ipc_token_file
        .as_deref()
        .map(|f| {
            std::fs::read_to_string(f)
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    match osiris_ipc_client::IpcClient::connect(
        addr,
        &token,
        std::time::Duration::from_millis(cfg.ipc_timeout_ms),
    ) {
        Ok(client) => {
            info!(
                godot = %client.godot_version,
                addr = %addr,
                "conectado al puente Godot"
            );
            Some(client)
        }
        Err(e) => {
            warn!(error = %e, "no se pudo conectar al puente Godot; herramientas Godot no disponibles");
            None
        }
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let cli = match config::parse_args(&argv) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("error: {e}\n{}", config::USAGE);
            return ExitCode::from(2);
        }
    };

    if cli.help {
        println!("{}", config::USAGE);
        return ExitCode::SUCCESS;
    }

    let cfg = match config::load(&cli, |k| std::env::var(k).ok()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    if let Err(e) = osiris_telemetry::init(
        &cfg.log_level,
        cfg.audit_file.as_deref().map(std::path::Path::new),
    ) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let ipc = connect_ipc(&cfg);

    info!("servidor MCP stdio iniciado");
    let mut server = osiris_mcp::server::Server::new(ipc, cfg.capability_profile);
    if let Err(e) = osiris_mcp::server::run_server(&mut server) {
        error!(error = %e, "el servidor stdio terminó con error");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
