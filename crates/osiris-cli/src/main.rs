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

    println!(
        "osiris-server: bind_addr={} log_level={} (servidor MCP: pendiente, Fase B)",
        cfg.bind_addr, cfg.log_level
    );
    ExitCode::SUCCESS
}
