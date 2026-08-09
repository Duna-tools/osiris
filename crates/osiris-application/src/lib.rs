//! Servicios de aplicación: orquestación, autorización, idempotencia y registro
//! de herramientas (osiris-godot-mcp-docs: application layer).
//!
//! Capa entre los adaptadores (MCP, CLI) y el dominio. Depende de `domain`
//! (errores + port `GodotPort`) y de `policy` (ADR-001: dependencias hacia
//! adentro). El registro de herramientas (ADR-008) declara nombre, operación
//! IPC, efecto y handler en un solo lugar.

use osiris_domain::{DomainError, GodotPort};
use osiris_policy::Profile;
use serde_json::Value;
use std::collections::VecDeque;

/// Efecto de una herramienta: distingue lectura (cacheable) de mutación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
}

/// Handler de una herramienta. Recibe el spec para reusar `op` de forma genérica.
type ToolHandler = fn(&ToolSpec, &Value, Option<&mut dyn GodotPort>) -> Result<Value, DomainError>;

/// Descriptor de herramienta registrada (ADR-008).
pub struct ToolSpec {
    pub name: &'static str,
    /// Operación IPC hacia el puente, o `None` si la tool es local al núcleo.
    op: Option<&'static str>,
    effect: Effect,
    handler: ToolHandler,
}

/// Registro estático de herramientas (factory method por tool).
static TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "osiris_diagnose",
        op: None,
        effect: Effect::Read,
        handler: local_diagnose,
    },
    ToolSpec {
        name: "godot_diagnose",
        op: Some("get_version"),
        effect: Effect::Read,
        handler: godot_diagnose,
    },
    ToolSpec {
        name: "godot.scene.active",
        op: Some("get_active_scene"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.nodes",
        op: Some("list_nodes"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.properties",
        op: Some("get_property"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.resources",
        op: Some("list_resources"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.editor.errors",
        op: Some("get_editor_errors"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.create_node",
        op: Some("create_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.remove_node",
        op: Some("remove_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.set_property",
        op: Some("set_property"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.save",
        op: Some("save_scene"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.run",
        op: Some("run_project"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.stop",
        op: Some("stop_project"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.run.status",
        op: Some("get_run_status"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.run.output",
        op: Some("get_run_output"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.assert.property",
        op: Some("assert_property"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.script.validate",
        op: Some("validate_script"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
];

/// Cache de idempotencia: evita re-ejecutar lecturas repetidas (error-model.md §4).
const MAX_CACHE_ENTRIES: usize = 128;

pub struct IdempotencyCache {
    entries: VecDeque<(String, Value)>,
}

impl std::fmt::Debug for IdempotencyCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdempotencyCache")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl Default for IdempotencyCache {
    fn default() -> Self {
        Self::new()
    }
}

impl IdempotencyCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CACHE_ENTRIES),
        }
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find_map(|(k, v)| (k == key).then_some(v))
    }

    /// Inserta o actualiza una entrada; descarta la más antigua al llenar.
    fn put(&mut self, key: String, value: Value) {
        self.entries.retain(|(k, _)| k != &key);
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back((key, value));
    }
}

fn cache_key(tool: &str, params: &Value) -> String {
    format!(
        "{tool}:{}",
        serde_json::to_string(params).unwrap_or_default()
    )
}

fn find_tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Despacha el `tools/call` hacia la herramienta registrada.
///
/// `profile` determina las capacidades del cliente (ADR-005).
///
/// # Errors
/// Devuelve `Err` si la herramienta no está autorizada, es desconocida o el IPC falla.
pub fn handle_tool(
    name: &str,
    params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    profile: Profile,
    cache: &mut IdempotencyCache,
) -> Result<Value, DomainError> {
    osiris_policy::check(profile, name)?;
    let spec = find_tool(name)
        .ok_or_else(|| DomainError::Validation(format!("herramienta desconocida: {name}")))?;

    if spec.effect == Effect::Write {
        (spec.handler)(spec, params, ipc)
    } else {
        let key = cache_key(name, params);
        if let Some(cached) = cache.get(&key) {
            return Ok(cached.clone());
        }
        let result = (spec.handler)(spec, params, ipc)?;
        cache.put(key, result.clone());
        Ok(result)
    }
}

/// Handler genérico: ejecuta la operación IPC y devuelve el resultado plano.
fn ipc_plain(
    spec: &ToolSpec,
    params: &Value,
    ipc: Option<&mut dyn GodotPort>,
) -> Result<Value, DomainError> {
    ipc_tool(
        ipc,
        spec.op.expect("op requerido"),
        params.clone(),
        plain_result(),
    )
}

/// Handler local: diagnóstico del propio servidor (sin puente).
fn local_diagnose(
    _spec: &ToolSpec,
    _params: &Value,
    _ipc: Option<&mut dyn GodotPort>,
) -> Result<Value, DomainError> {
    Ok(osiris_diagnose())
}

/// Handler específico de `godot_diagnose`: incluye la versión del handshake.
fn godot_diagnose(
    spec: &ToolSpec,
    _params: &Value,
    ipc: Option<&mut dyn GodotPort>,
) -> Result<Value, DomainError> {
    ipc_tool(
        ipc,
        spec.op.expect("op requerido"),
        Value::Null,
        |client, result| {
            serde_json::json!({
                "godot_version": client.godot_version(),
                "godot_details": result,
            })
        },
    )
}

fn osiris_diagnose() -> Value {
    let info = serde_json::json!({
        "name": "osiris",
        "version": env!("CARGO_PKG_VERSION"),
        "protocolVersions": ["2025-06-18", "2025-03-26", "2024-11-05"],
        "transport": "stdio",
    });
    serde_json::json!({
        "content": [{ "type": "text", "text": info.to_string() }],
        "isError": false,
    })
}

fn plain_result() -> fn(&dyn GodotPort, &Value) -> Value {
    |_, result| result.clone()
}

/// Ejecuta una orden en el puente y envuelve el resultado como texto MCP.
///
/// # Errors
/// Devuelve `Err` si el IPC no está configurado o la llamada al puente falla.
fn ipc_tool(
    ipc: Option<&mut dyn GodotPort>,
    op: &str,
    params: Value,
    transform: impl FnOnce(&dyn GodotPort, &Value) -> Value,
) -> Result<Value, DomainError> {
    let client = ipc.ok_or_else(|| {
        DomainError::Ipc("el puente Godot no está configurado (usa --ipc-addr)".to_string())
    })?;
    let result = client.call(op, params)?;
    let payload = transform(client, &result);
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": payload.to_string() }],
        "isError": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_unique() {
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len(), "ADR-008: nombres duplicados");
    }

    #[test]
    fn effect_classifies_write_vs_read() {
        assert_eq!(
            find_tool("godot.scene.create_node").unwrap().effect,
            Effect::Write
        );
        assert_eq!(
            find_tool("godot.scene.set_property").unwrap().effect,
            Effect::Write
        );
        assert_eq!(find_tool("godot.run").unwrap().effect, Effect::Write);
        assert_eq!(
            find_tool("godot.scene.active").unwrap().effect,
            Effect::Read
        );
        assert_eq!(find_tool("osiris_diagnose").unwrap().effect, Effect::Read);
    }

    #[test]
    fn unknown_tool_errors() {
        assert!(find_tool("no.existe").is_none());
    }
}
