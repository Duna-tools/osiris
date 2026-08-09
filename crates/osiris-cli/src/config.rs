//! Configuración del binario `osiris-server`.
//!
//! Precedencia (deployment-model.md §3): argumentos CLI > variables de entorno
//! `OSIRIS_*` > archivo TOML (`--config` o `OSIRIS_CONFIG`) > valores por defecto.
//! ADR-006: la dirección por defecto es loopback; nunca se expone en red sin
//! configuración explícita.

const DEFAULT_LOG_LEVEL: &str = "info";

use serde::Deserialize;

use osiris_policy::Profile;

/// Configuración resuelta tras aplicar toda la cadena de precedencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub log_level: String,
    pub ipc_addr: Option<String>,
    pub ipc_token_file: Option<String>,
    pub ipc_timeout_ms: u64,
    pub capability_profile: Profile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    log_level: Option<String>,
    ipc_addr: Option<String>,
    ipc_token_file: Option<String>,
    ipc_timeout_ms: Option<u64>,
}

/// Argumentos CLI ya parseados (todos opcionales salvo `--help`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub config_path: Option<String>,
    pub log_level: Option<String>,
    pub ipc_addr: Option<String>,
    pub ipc_token_file: Option<String>,
    pub ipc_timeout_ms: Option<u64>,
    pub capability_profile: Option<String>,
    pub help: bool,
}

/// Texto de ayuda del binario.
pub const USAGE: &str = "Uso: osiris-server [opciones]

Opciones:
  --config <ruta>       Archivo TOML de configuración (o OSIRIS_CONFIG)
  --log-level <nivel>   Nivel de log (o OSIRIS_LOG_LEVEL) [defecto: info]
  --ipc-addr <addr>     Dirección del puente Godot (o OSIRIS_IPC_ADDR)
  --ipc-token-file <f>  Archivo con el token del puente (o OSIRIS_IPC_TOKEN_FILE)
  --ipc-timeout-ms <n>  Timeout de operaciones IPC en milisegundos (o OSIRIS_IPC_TIMEOUT_MS) [defecto: 3000]
  --capability-profile <p>  Perfil de capacidades: read|edit|admin (o OSIRIS_CAPABILITY_PROFILE) [defecto: read]
  -h, --help            Muestra esta ayuda";

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
            "--config"
            | "--log-level"
            | "--ipc-addr"
            | "--ipc-token-file"
            | "--ipc-timeout-ms"
            | "--capability-profile" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{flag} requiere un valor"))?
                    .clone();
                match flag {
                    "--config" => out.config_path = Some(value),
                    "--ipc-addr" => out.ipc_addr = Some(value),
                    "--ipc-token-file" => out.ipc_token_file = Some(value),
                    "--ipc-timeout-ms" => {
                        out.ipc_timeout_ms = Some(
                            value
                                .parse()
                                .map_err(|_| "--ipc-timeout-ms debe ser un número")?,
                        )
                    }
                    "--capability-profile" => out.capability_profile = Some(value),
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
        log_level: DEFAULT_LOG_LEVEL.to_string(),
        ipc_addr: None,
        ipc_token_file: None,
        ipc_timeout_ms: 3000,
        capability_profile: Profile::Read,
    };

    let config_path = args.config_path.clone().or_else(|| env("OSIRIS_CONFIG"));
    if let Some(path) = config_path {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("no se pudo leer {path}: {e}"))?;
        let file: FileConfig =
            toml::from_str(&text).map_err(|e| format!("TOML inválido en {path}: {e}"))?;
        if let Some(v) = file.log_level {
            cfg.log_level = v;
        }
        if cfg.ipc_addr.is_none() {
            cfg.ipc_addr = file.ipc_addr;
        }
        if cfg.ipc_token_file.is_none() {
            cfg.ipc_token_file = file.ipc_token_file;
        }
        if cfg.ipc_timeout_ms == 3000 {
            cfg.ipc_timeout_ms = file.ipc_timeout_ms.unwrap_or(3000);
        }
    }

    if let Some(v) = env("OSIRIS_LOG_LEVEL") {
        cfg.log_level = v;
    }
    if let Some(v) = env("OSIRIS_IPC_ADDR") {
        cfg.ipc_addr = Some(v);
    }
    if let Some(v) = env("OSIRIS_IPC_TOKEN_FILE") {
        cfg.ipc_token_file = Some(v);
    }
    if let Some(v) = env("OSIRIS_IPC_TIMEOUT_MS") {
        cfg.ipc_timeout_ms = v
            .parse()
            .map_err(|e| format!("OSIRIS_IPC_TIMEOUT_MS: {e}"))?;
    }
    if let Some(v) = env("OSIRIS_CAPABILITY_PROFILE") {
        cfg.capability_profile = v
            .parse()
            .map_err(|e| format!("OSIRIS_CAPABILITY_PROFILE: {e}"))?;
    }

    if let Some(v) = &args.log_level {
        cfg.log_level.clone_from(v);
    }
    if let Some(v) = &args.ipc_addr {
        cfg.ipc_addr = Some(v.clone());
    }
    if let Some(v) = &args.ipc_token_file {
        cfg.ipc_token_file = Some(v.clone());
    }
    if let Some(v) = &args.ipc_timeout_ms {
        cfg.ipc_timeout_ms = *v;
    }
    if let Some(v) = &args.capability_profile {
        cfg.capability_profile = v
            .parse()
            .map_err(|e| format!("--capability-profile: {e}"))?;
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
        parse_args(
            &list
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    #[test]
    fn defaults_are_info() {
        let cfg = load(&CliArgs::default(), no_env).unwrap();
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn args_beat_env_and_env_beats_file() {
        let dir = std::env::temp_dir().join(format!("osiris-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("osiris.toml");
        std::fs::write(&path, "log_level = \"warn\"\n").unwrap();
        let path_str = path.to_string_lossy().to_string();

        let cfg = load(&args(&["--config", &path_str]), no_env).unwrap();
        assert_eq!(cfg.log_level, "warn");

        let env = |k: &str| (k == "OSIRIS_LOG_LEVEL").then(|| "debug".to_string());
        let cfg = load(&args(&["--config", &path_str]), env).unwrap();
        assert_eq!(cfg.log_level, "debug");

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

    #[test]
    fn ipc_timeout_default_and_override() {
        let cfg = load(&CliArgs::default(), no_env).unwrap();
        assert_eq!(cfg.ipc_timeout_ms, 3000);
        let cfg = load(&args(&["--ipc-timeout-ms", "1500"]), no_env).unwrap();
        assert_eq!(cfg.ipc_timeout_ms, 1500);
    }
}
