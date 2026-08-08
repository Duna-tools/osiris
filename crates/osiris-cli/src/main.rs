//! CLI: punto de entrada del binario osiris-server (deployment-model.md).
//!
//! Por ahora: parsea argumentos, resuelve la configuración con la precedencia
//! documentada y muestra la configuración efectiva. El servidor MCP se
//! conectará en fases posteriores (roadmap Fase B).

mod config;

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let args = match config::parse_args(&argv) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("error: {e}\n{}", config::USAGE);
            return ExitCode::from(2);
        }
    };

    if args.help {
        println!("{}", config::USAGE);
        return ExitCode::SUCCESS;
    }

    let cfg = match config::load(&args, |k| std::env::var(k).ok()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    if let Err(e) = osiris_telemetry::init(&cfg.log_level) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let ipc = match &cfg.ipc_addr {
        Some(addr) => {
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
                std::time::Duration::from_secs(3),
            ) {
                Ok(client) => {
                    tracing::info!(godot = %client.godot_version, "conectado al puente Godot en {addr}");
                    Some(client)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "no se pudo conectar al puente Godot; herramientas Godot no disponibles");
                    None
                }
            }
        }
        None => None,
    };

    tracing::info!(bind_addr = %cfg.bind_addr, "servidor MCP stdio iniciado");
    let mut server = osiris_mcp::server::Server::new(ipc, cfg.capability_profile);
    if let Err(e) = osiris_mcp::server::run_server(&mut server) {
        tracing::error!(error = %e, "el servidor stdio terminó con error");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
