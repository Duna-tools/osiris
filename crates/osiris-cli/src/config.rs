//! Configuración del binario `osiris-server`.
//!
//! Precedencia (deployment-model.md §3): argumentos CLI > variables de entorno
//! `OSIRIS_*` > archivo TOML (`--config` o `OSIRIS_CONFIG`) > valores por defecto.
//! ADR-006: la dirección por defecto es loopback; nunca se expone en red sin
//! configuración explícita.

use serde::Deserialize;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
const DEFAULT_LOG_LEVEL: &str = "info";

/// Configuración resuelta tras aplicar toda la cadena de precedencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub bind_addr: String,
    pub log_level: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    bind_addr: Option<String>,
    log_level: Option<String>,
}

/// Argumentos CLI ya parseados (todos opcionales salvo `--help`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub config_path: Option<String>,
    pub bind_addr: Option<String>,
    pub log_level: Option<String>,
    pub help: bool,
}

/// Texto de ayuda del binario.
pub const USAGE: &str = "Uso: osiris-server [opciones]

Opciones:
  --config <ruta>     Archivo TOML de configuración (o OSIRIS_CONFIG)
  --bind <addr>       Dirección de escucha (o OSIRIS_BIND_ADDR) [defecto: 127.0.0.1:3000]
  --log-level <nivel> Nivel de log (o OSIRIS_LOG_LEVEL) [defecto: info]
  -h, --help          Muestra esta ayuda";

/// Parsea argumentos ya separados (sin el nombre del programa).
///
/// # Errores
/// Devuelve `Err` si un flag conocido carece de valor o aparece un argumento
/// desconocido. El binario debe responder con código de salida 2 y el mensaje.
pub fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut out = CliArgs::default();
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        match flag {
            "-h" | "--help" => out.help = true,
            "--config" | "--bind" | "--log-level" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{flag} requiere un valor"))?
                    .clone();
                match flag {
                    "--config" => out.config_path = Some(value),
                    "--bind" => out.bind_addr = Some(value),
                    _ => out.log_level = Some(value),
                }
                i += 1;
            }
            other => return Err(format!("argumento desconocido: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

/// Resuelve la configuración aplicando la cadena de precedencia completa.
///
/// `env` es el acceso a variables de entorno (inyectado para poder probar la
/// precedencia sin mutar el entorno del proceso).
///
/// # Errores
/// Devuelve `Err` si el archivo de configuración no existe o no es TOML válido.
pub fn load(args: &CliArgs, env: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
    let mut cfg = Config {
        bind_addr: DEFAULT_BIND_ADDR.to_string(),
        log_level: DEFAULT_LOG_LEVEL.to_string(),
    };

    let config_path = args.config_path.clone().or_else(|| env("OSIRIS_CONFIG"));
    if let Some(path) = config_path {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("no se pudo leer {path}: {e}"))?;
        let file: FileConfig =
            toml::from_str(&text).map_err(|e| format!("TOML inválido en {path}: {e}"))?;
        if let Some(v) = file.bind_addr {
            cfg.bind_addr = v;
        }
        if let Some(v) = file.log_level {
            cfg.log_level = v;
        }
    }

    if let Some(v) = env("OSIRIS_BIND_ADDR") {
        cfg.bind_addr = v;
    }
    if let Some(v) = env("OSIRIS_LOG_LEVEL") {
        cfg.log_level = v;
    }

    if let Some(v) = &args.bind_addr {
        cfg.bind_addr = v.clone();
    }
    if let Some(v) = &args.log_level {
        cfg.log_level = v.clone();
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn args(list: &[&str]) -> CliArgs {
        parse_args(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn defaults_are_loopback_and_info() {
        let cfg = load(&CliArgs::default(), no_env).unwrap();
        assert_eq!(cfg.bind_addr, "127.0.0.1:3000");
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn args_beat_env_and_env_beats_file() {
        let dir = std::env::temp_dir().join(format!("osiris-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("osiris.toml");
        std::fs::write(&path, "bind_addr = \"10.0.0.1:1\"\nlog_level = \"warn\"\n").unwrap();
        let path_str = path.to_string_lossy().to_string();

        // Archivo solo.
        let cfg = load(&args(&["--config", &path_str]), no_env).unwrap();
        assert_eq!(cfg.bind_addr, "10.0.0.1:1");
        assert_eq!(cfg.log_level, "warn");

        // Env gana al archivo.
        let env = |k: &str| (k == "OSIRIS_LOG_LEVEL").then(|| "debug".to_string());
        let cfg = load(&args(&["--config", &path_str]), env).unwrap();
        assert_eq!(cfg.bind_addr, "10.0.0.1:1");
        assert_eq!(cfg.log_level, "debug");

        // Arg gana a env.
        let cfg = load(&args(&["--config", &path_str, "--log-level", "error"]), env).unwrap();
        assert_eq!(cfg.log_level, "error");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_flag_and_missing_value_are_errors() {
        let err = parse_args(&["--nope".to_string()]).unwrap_err();
        assert!(err.contains("desconocido"));
        let err = parse_args(&["--config".to_string()]).unwrap_err();
        assert!(err.contains("requiere un valor"));
    }

    #[test]
    fn help_flag_is_recognized() {
        assert!(args(&["--help"]).help);
        assert!(args(&["-h"]).help);
    }
}
