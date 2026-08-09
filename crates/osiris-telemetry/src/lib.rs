//! Telemetría: logs estructurados, métricas, trazas y redacción.
//!
//! Base de logging sobre `tracing` (observability.md §2): nivel configurable
//! con la sintaxis de `EnvFilter` (`info`, `osiris=debug`, etc.).
//!
//! La redacción de secretos se implementará cuando exista el primero (token
//! local, T-024); hoy no hay ningún valor que redactar.

use std::fs::File;
use std::path::Path;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

pub use tracing::{error, info, warn};

/// Construye el filtro de nivel a partir de la configuración.
///
/// Acepta un nivel simple (`trace|debug|info|warn|error|off`) o directivas
/// (`osiris=debug`). `EnvFilter::try_new` es permisivo y trataría cualquier
/// cadena como nombre de target, así que los valores simples se validan antes
/// contra `LevelFilter` para que un typo falle en arranque y no silencie logs.
///
/// # Errors
/// Devuelve `Err` si la cadena no es un nivel o una directiva válida.
fn filter(log_level: &str) -> Result<EnvFilter, String> {
    let simple = !log_level.contains(['=', ',']);
    if simple && LevelFilter::from_str(log_level).is_err() {
        return Err(format!(
            "nivel de log inválido '{log_level}' (use trace|debug|info|warn|error|off o directivas tipo 'osiris=debug')"
        ));
    }
    EnvFilter::try_new(log_level).map_err(|e| format!("nivel de log inválido '{log_level}': {e}"))
}

/// Inicializa el suscriptor global de logs (formato texto, con target y nivel).
///
/// Escribe en **stderr**: stdout es el canal del protocolo MCP por stdio y un
/// log allí corrompería el framing JSON-RPC.
///
/// Si `audit_file` es `Some`, además persiste los eventos del target
/// `osiris_audit` (RF-14, TD-004) en ese archivo en formato JSON por línea.
///
/// Debe llamarse una sola vez al arrancar el proceso.
///
/// # Errors
/// Devuelve `Err` si el nivel es inválido, el archivo de auditoría no se puede
/// abrir, o el suscriptor global ya existe.
pub fn init(log_level: &str, audit_file: Option<&Path>) -> Result<(), String> {
    let filter = filter(log_level)?;
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);
    match audit_file {
        Some(path) => {
            let file = File::create(path)
                .map_err(|e| format!("no se pudo abrir el archivo de auditoría {path:?}: {e}"))?;
            let audit_layer = tracing_subscriber::fmt::layer()
                .with_writer(file)
                .with_target(true)
                .json()
                .with_filter(
                    tracing_subscriber::filter::Targets::new()
                        .with_target("osiris_audit", tracing::Level::INFO),
                );
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(audit_layer)
                .try_init()
                .map_err(|e| format!("no se pudo inicializar el log: {e}"))
        }
        None => tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .try_init()
            .map_err(|e| format!("no se pudo inicializar el log: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_level() {
        assert!(filter("bogus!!").is_err());
        assert!(filter("verboso").is_err());
    }

    #[test]
    fn accepts_levels_and_directives() {
        assert!(filter("info").is_ok());
        assert!(filter("osiris=debug").is_ok());
    }

    // `init` muta estado global del proceso: se verifica en vivo al arrancar
    // el binario, no en tests unitarios (una sola inicialización por proceso).
}
