//! Telemetría: logs estructurados, métricas, trazas y redacción.
//!
//! Base de logging sobre `tracing` (observability.md §2): nivel configurable
//! con la sintaxis de `EnvFilter` (`info`, `osiris=debug`, etc.).
//!
//! La redacción de secretos se implementará cuando exista el primero (token
//! local, T-024); hoy no hay ningún valor que redactar.

use std::str::FromStr;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// Construye el filtro de nivel a partir de la configuración.
///
/// Acepta un nivel simple (`trace|debug|info|warn|error|off`) o directivas
/// (`osiris=debug`). `EnvFilter::try_new` es permisivo y trataría cualquier
/// cadena como nombre de target, así que los valores simples se validan antes
/// contra `LevelFilter` para que un typo falle en arranque y no silencie logs.
///
/// # Errores
/// Devuelve `Err` si la cadena no es un nivel ni una directiva válida.
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
/// Debe llamarse una sola vez al arrancar el proceso.
///
/// # Errores
/// Devuelve `Err` si el nivel es inválido o el suscriptor global ya existe.
pub fn init(log_level: &str) -> Result<(), String> {
    let filter = filter(log_level)?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init()
        .map_err(|e| format!("no se pudo inicializar el log: {e}"))
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
