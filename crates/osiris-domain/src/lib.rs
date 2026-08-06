//! Modelo de errores estable del dominio (docs/05-architecture/error-model.md).
//!
//! Categorías externas estables, con código, mensaje seguro y política de reintento.
//! La causa interna viaja en logs protegidos, nunca en el mensaje externo.

/// Categorías de error externo (error-model.md §2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    /// Esquema o tipo inválido. No reintentable.
    #[error("validation: {0}")]
    Validation(String),
    /// Credencial ausente o inválida. No reintentable salvo nueva credencial.
    #[error("authentication")]
    Authentication,
    /// Capacidad insuficiente. No reintentable.
    #[error("authorization: {0}")]
    Authorization(String),
    /// Versión no soportada. No reintentable.
    #[error("compatibility: {0}")]
    Compatibility(String),
    /// Cola llena. Reintentable con backoff.
    #[error("saturation: {0}")]
    Saturation(String),
    /// Operación excedida. Reintento depende de idempotencia.
    #[error("timeout: {0}")]
    Timeout(String),
    /// El motor (Godot) reportó un error. Reintento tras refrescar o nunca.
    #[error("godot: {0}")]
    Godot(String),
    /// Canal IPC desconectado. Reintentable para lecturas seguras.
    #[error("ipc: {0}")]
    Ipc(String),
    /// El estado observado cambió entre solicitud y ejecución.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Invariante rota o fallo inesperado. No reintento automático.
    #[error("internal: {0}")]
    Internal(String),
}

impl DomainError {
    /// Código estable, seguro de exponer a un cliente MCP.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation",
            Self::Authentication => "authentication",
            Self::Authorization(_) => "authorization",
            Self::Compatibility(_) => "compatibility",
            Self::Saturation(_) => "saturation",
            Self::Timeout(_) => "timeout",
            Self::Godot(_) => "godot",
            Self::Ipc(_) => "ipc",
            Self::Conflict(_) => "conflict",
            Self::Internal(_) => "internal",
        }
    }

    /// ¿La operación puede reintentarse tal cual?
    pub fn retryable(&self) -> bool {
        match self {
            Self::Saturation(_) => true,
            Self::Ipc(_) => true,
            Self::Conflict(_) => true,
            Self::Timeout(_)
            | Self::Godot(_)
            | Self::Validation(_)
            | Self::Authentication
            | Self::Authorization(_)
            | Self::Compatibility(_)
            | Self::Internal(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_is_retryable_and_validation_is_not() {
        let s = DomainError::Saturation("queue full".into());
        assert!(s.retryable());
        assert_eq!(s.code(), "saturation");
        let v = DomainError::Validation("bad schema".into());
        assert!(!v.retryable());
        assert_eq!(v.code(), "validation");
    }

    #[test]
    fn every_variant_has_a_stable_code() {
        let cases = [
            (DomainError::Validation("".into()), "validation"),
            (DomainError::Authentication, "authentication"),
            (DomainError::Authorization("".into()), "authorization"),
            (DomainError::Compatibility("".into()), "compatibility"),
            (DomainError::Saturation("".into()), "saturation"),
            (DomainError::Timeout("".into()), "timeout"),
            (DomainError::Godot("".into()), "godot"),
            (DomainError::Ipc("".into()), "ipc"),
            (DomainError::Conflict("".into()), "conflict"),
            (DomainError::Internal("".into()), "internal"),
        ];
        for (e, code) in cases {
            assert_eq!(e.code(), code, "código estable para {e}");
        }
    }
}
