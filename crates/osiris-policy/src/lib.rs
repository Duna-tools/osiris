//! Motor de políticas: deny-by-default, autorización por capacidades y límites.
//!
//! ADR-005: el servidor arranca con un perfil (read, edit, admin). Cada
//! herramienta declara la capacidad que requiere. `tools/list` omite las
//! herramientas para las que el perfil no tiene capacidad.

use osiris_domain::DomainError;
use std::str::FromStr;

/// Capacidades atómicas del modelo ADR-005.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    GodotSceneWrite,
    GodotRun,
}

/// Perfil de capacidades con el que arranca el servidor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Solo lectura: herramientas de inspección.
    Read,
    /// Lectura y modificación segura de escenas (crear/eliminar/editar nodos).
    Edit,
    /// Perfil completo: ejecución de proyectos, modificaciones sin restricciones.
    Admin,
}

impl Profile {
    const fn mask(self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Edit => 1 << Capability::GodotSceneWrite as u8,
            Self::Admin => 0xFF,
        }
    }

    const fn has(self, cap: Capability) -> bool {
        self.mask() & (1 << cap as u8) != 0
    }
}

impl FromStr for Profile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read" => Ok(Self::Read),
            "edit" => Ok(Self::Edit),
            "admin" => Ok(Self::Admin),
            other => Err(format!(
                "perfil desconocido '{other}' (use read|edit|admin)"
            )),
        }
    }
}

/// La capacidad que una herramienta necesita, o `None` si es de solo lectura.
#[must_use]
pub fn required(tool_name: &str) -> Option<Capability> {
    match tool_name {
        "godot.scene.create_node"
        | "godot.scene.remove_node"
        | "godot.scene.set_property"
        | "godot.scene.save" => Some(Capability::GodotSceneWrite),
        "godot.run" | "godot.stop" => Some(Capability::GodotRun),
        _ => None,
    }
}

/// Verifies the profile has the capability to execute the tool.
///
/// # Errors
/// `Authorization` if the profile does not grant the required capability.
pub fn check(profile: Profile, tool: &str) -> Result<(), DomainError> {
    match required(tool) {
        None => Ok(()),
        Some(cap) if profile.has(cap) => Ok(()),
        Some(_cap) => Err(DomainError::Authorization(format!(
            "el perfil {profile:?} no tiene capacidad para {tool}"
        ))),
    }
}

/// Indica si una herramienta debe listarse dado el perfil actual.
#[must_use]
pub fn visible_in(profile: Profile, tool: &str) -> bool {
    required(tool).is_none_or(|cap| profile.has(cap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_profile_rejects_write() {
        let r = check(Profile::Read, "godot.scene.set_property");
        assert!(r.is_err());
        assert!(matches!(r.unwrap_err(), DomainError::Authorization(_)));
    }

    #[test]
    fn edit_profile_allows_write() {
        assert!(check(Profile::Edit, "godot.scene.set_property").is_ok());
        assert!(check(Profile::Edit, "godot.run").is_err());
    }

    #[test]
    fn admin_allows_everything() {
        assert!(check(Profile::Admin, "godot.scene.set_property").is_ok());
        assert!(check(Profile::Admin, "godot.run").is_ok());
    }

    #[test]
    fn read_tools_need_no_capability() {
        assert_eq!(required("godot.scene.active"), None);
        assert_eq!(required("osiris_diagnose"), None);
    }

    #[test]
    fn profile_parsing() {
        assert_eq!("read".parse::<Profile>().unwrap(), Profile::Read);
        assert_eq!("edit".parse::<Profile>().unwrap(), Profile::Edit);
        assert_eq!("admin".parse::<Profile>().unwrap(), Profile::Admin);
        assert!("root".parse::<Profile>().is_err());
    }

    #[test]
    fn visible_in_filters_by_capability() {
        assert!(visible_in(Profile::Read, "godot.scene.nodes"));
        assert!(!visible_in(Profile::Read, "godot.scene.set_property"));
        assert!(visible_in(Profile::Edit, "godot.scene.set_property"));
        assert!(!visible_in(Profile::Edit, "godot.run"));
        assert!(visible_in(Profile::Admin, "godot.run"));
    }
}
