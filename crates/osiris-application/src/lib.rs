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

/// Reexport del efecto (dominio rico, ADR-001).
pub use osiris_domain::Effect;

/// Handler de una herramienta. `ipc` es el puente; `cache` y `profile` permiten
/// a tools compuestas (planes) orquestar otras tools (RF-10). `ipc` y `cache`
/// tienen vidas independientes para poder reborrarlas por paso en un plan.
type ToolHandler = for<'a, 'b> fn(
    &ToolSpec,
    &Value,
    Option<&'a mut dyn GodotPort>,
    &'b mut IdempotencyCache,
    Profile,
) -> Result<Value, DomainError>;

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
    ToolSpec {
        name: "godot.describe",
        op: Some("describe_class"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.signal.list",
        op: Some("list_signals"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.signal.connect",
        op: Some("connect_signal"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.signal.disconnect",
        op: Some("disconnect_signal"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.asset.list",
        op: Some("asset_list"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.asset.reimport",
        op: Some("asset_reimport"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.resource.assign",
        op: Some("resource_assign"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "plan.validate",
        op: None,
        effect: Effect::Read,
        handler: plan_validate,
    },
    ToolSpec {
        name: "plan.execute",
        op: None,
        effect: Effect::Write,
        handler: plan_execute,
    },
    ToolSpec {
        name: "skill.list",
        op: None,
        effect: Effect::Read,
        handler: skill_list,
    },
    ToolSpec {
        name: "skill.get",
        op: None,
        effect: Effect::Read,
        handler: skill_get,
    },
    ToolSpec {
        name: "osiris.capabilities",
        op: None,
        effect: Effect::Read,
        handler: capabilities_handler,
    },
    ToolSpec {
        name: "godot.runtime.tree",
        op: Some("runtime_tree"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.runtime.metrics",
        op: Some("runtime_metrics"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
];

/// Skill: conocimiento declarativo de cómo resolver una intención con tools
/// ya autorizadas (RF-11, vault 07 §5). No añade privilegios.
struct Skill {
    name: &'static str,
    description: &'static str,
    /// Tools que la skill usa (deben estar en TOOLS).
    uses: &'static [&'static str],
    /// Conocimiento de apoyo para el LLM.
    knowledge: &'static [&'static str],
}

static SKILLS: &[Skill] = &[
    Skill {
        name: "setup_2d_player",
        description: "Crea un player 2D básico (CharacterBody2D con Sprite2D) en la escena activa.",
        uses: &[
            "godot.describe",
            "godot.scene.create_node",
            "godot.scene.set_property",
            "godot.resource.assign",
            "godot.script.validate",
            "godot.signal.connect",
        ],
        knowledge: &[
            "CharacterBody2D es la base de movimiento para jugadores 2D",
            "El movimiento usa velocity + move_and_slide() en _physics_process",
            "El InputMap requiere acciones de entrada antes de usar Input.get_vector()",
            "Los scripts se validan con godot.script.validate antes de escribir en disco",
        ],
    },
    Skill {
        name: "create_ui_button",
        description: "Crea un botón UI con señal pressed conectada a un método del nodo raíz.",
        uses: &[
            "godot.describe",
            "godot.scene.create_node",
            "godot.signal.connect",
        ],
        knowledge: &[
            "Button extiende BaseButton; la señal pressed se emite al hacer clic",
            "El método destino debe existir en el nodo receptor",
            "Connectar con deferred=false ejecuta la llamada de inmediato",
        ],
    },
];

fn skill_list(
    _spec: &ToolSpec,
    _params: &Value,
    _ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    _profile: Profile,
) -> Result<Value, DomainError> {
    let items: Vec<Value> = SKILLS
        .iter()
        .map(|s| serde_json::json!({ "name": s.name, "description": s.description }))
        .collect();
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": serde_json::json!({ "skills": items }).to_string() }],
        "isError": false,
    }))
}

fn skill_get(
    _spec: &ToolSpec,
    params: &Value,
    _ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    _profile: Profile,
) -> Result<Value, DomainError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| DomainError::Validation("parámetro 'name' requerido".to_string()))?;
    let skill = SKILLS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| DomainError::Validation(format!("skill desconocida: {name}")))?;
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": serde_json::json!({
            "name": skill.name,
            "description": skill.description,
            "uses": skill.uses,
            "knowledge": skill.knowledge,
        }).to_string() }],
        "isError": false,
    }))
}

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

/// Efecto declarado de una herramienta, para auditoría (RF-14).
#[must_use]
pub fn effect_of(tool: &str) -> Option<Effect> {
    find_tool(tool).map(|t| t.effect)
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
        let ipc_short = ipc.map(|i| &mut *i);
        let cache_short: &mut IdempotencyCache = &mut *cache;
        (spec.handler)(spec, params, ipc_short, cache_short, profile)
    } else {
        let key = cache_key(name, params);
        if let Some(cached) = cache.get(&key) {
            return Ok(cached.clone());
        }
        let ipc_short = ipc.map(|i| &mut *i);
        let cache_short: &mut IdempotencyCache = &mut *cache;
        let result = (spec.handler)(spec, params, ipc_short, cache_short, profile)?;
        cache.put(key, result.clone());
        Ok(result)
    }
}

/// Handler genérico: ejecuta la operación IPC y devuelve el resultado plano.
fn ipc_plain(
    spec: &ToolSpec,
    params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    _profile: Profile,
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
    _cache: &mut IdempotencyCache,
    _profile: Profile,
) -> Result<Value, DomainError> {
    Ok(osiris_diagnose())
}

/// Handler específico de `godot_diagnose`: incluye la versión del handshake.
fn godot_diagnose(
    spec: &ToolSpec,
    _params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    _profile: Profile,
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

/// Plan: lista de pasos con `tool`, `id` opcional y `params`. Los params
/// pueden referenciar el resultado de un paso previo con `$id.campo` (RF-10).
fn parse_plan(plan: &Value) -> Result<&[Value], DomainError> {
    let steps = plan
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| DomainError::Validation("plan.steps requerido (array)".to_string()))?;
    if steps.is_empty() {
        return Err(DomainError::Validation(
            "plan.steps no puede estar vacío".to_string(),
        ));
    }
    Ok(steps)
}

/// Sustituye `$id.campo` por el valor del paso `id` (resultado ya parseado).
fn resolve_ref(value: &Value, results: &std::collections::HashMap<String, Value>) -> Value {
    match value {
        Value::String(s) if s.starts_with('$') => {
            let path = &s[1..];
            let (id, field) = match path.split_once('.') {
                Some((id, f)) => (id, Some(f)),
                None => (path, None),
            };
            match results.get(id) {
                Some(v) => match field {
                    Some(f) => v.get(f).cloned().unwrap_or_else(|| value.clone()),
                    None => v.clone(),
                },
                None => value.clone(),
            }
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|i| resolve_ref(i, results)).collect())
        }
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), resolve_ref(v, results));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// Extrae el payload real de una tool (el `ok` en `content[0].text`) para
/// poder referenciarlo con `$id.campo` en planes. Si no hay wrapper, devuelve
/// el valor tal cual.
fn unwrap_tool_result(result: &Value) -> Value {
    result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| result.clone())
}

fn validate_steps(steps: &[Value]) -> Result<(), DomainError> {
    for (i, step) in steps.iter().enumerate() {
        let tool = step
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| DomainError::Validation(format!("step[{i}].tool requerido")))?;
        if find_tool(tool).is_none() {
            return Err(DomainError::Validation(format!(
                "step[{i}].tool desconocida: {tool}"
            )));
        }
        if step.get("params").is_some() && !step["params"].is_object() {
            return Err(DomainError::Validation(format!(
                "step[{i}].params debe ser objeto"
            )));
        }
    }
    Ok(())
}

/// Handler de `plan.validate`: valida estructura y herramientas sin ejecutar.
fn plan_validate(
    _spec: &ToolSpec,
    params: &Value,
    _ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    _profile: Profile,
) -> Result<Value, DomainError> {
    let plan = params
        .get("plan")
        .ok_or_else(|| DomainError::Validation("parámetro 'plan' requerido".to_string()))?;
    let steps = parse_plan(plan)?;
    validate_steps(steps)?;
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": serde_json::json!({
            "valid": true,
            "steps": steps.len(),
        }).to_string() }],
        "isError": false,
    }))
}

/// Handler de `plan.execute`: ejecuta pasos en orden, resolviendo `$id.campo`.
/// Un paso fallido aborta el plan y devuelve los receipts ya obtenidos.
fn plan_execute(
    _spec: &ToolSpec,
    params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    cache: &mut IdempotencyCache,
    profile: Profile,
) -> Result<Value, DomainError> {
    let plan = params
        .get("plan")
        .ok_or_else(|| DomainError::Validation("parámetro 'plan' requerido".to_string()))?;
    let steps = parse_plan(plan)?;
    validate_steps(steps)?;
    // `ipc` y `cache` ya llegan como reborrows del caller; `exec_steps` los
    // re-presta por paso con vidas cortas propias.
    exec_steps(steps, ipc, &mut *cache, profile)
}

/// Ejecuta los pasos con vidas cortas, desacopladas del `'a`/`'b` del handler.
fn exec_steps(
    steps: &[Value],
    mut ipc: Option<&mut dyn GodotPort>,
    cache: &mut IdempotencyCache,
    profile: Profile,
) -> Result<Value, DomainError> {
    let mut results: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    let mut receipts: Vec<Value> = Vec::with_capacity(steps.len());

    for (i, step) in steps.iter().enumerate() {
        let tool = step["tool"].as_str().unwrap();
        let id = step
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&format!("step{i}"))
            .to_string();
        let raw_params = step.get("params").cloned().unwrap_or(Value::Null);
        let resolved = resolve_ref(&raw_params, &results);
        // Reborrow manual por paso: `&mut **c` tiene la vida del body del loop,
        // no la del trait object (evita retener el borrow entre iteraciones).
        let ipc_ref: Option<&mut dyn GodotPort> = match &mut ipc {
            Some(c) => Some(&mut **c),
            None => None,
        };
        let result = handle_tool(tool, &resolved, ipc_ref, profile, cache)
            .map_err(|e| DomainError::Godot(format!("plan abortado en step[{i}] '{tool}': {e}")))?;
        // Guardamos el payload real (ok de la tool) para las referencias `$id.campo`,
        // no el wrapper MCP {content, isError}.
        let payload = unwrap_tool_result(&result);
        let id_clone = id.clone();
        results.insert(id.clone(), payload);
        receipts.push(serde_json::json!({
            "step": i,
            "tool": tool,
            "id": id_clone,
            "result": result,
        }));
    }

    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": serde_json::json!({
            "ok": true,
            "receipts": receipts,
        }).to_string() }],
        "isError": false,
    }))
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

/// Catálogo de capacidades (RF-12): qué capacidades y tools están disponibles
/// para la combinación actual de perfil, Godot y protocolo.
fn capabilities_handler(
    _spec: &ToolSpec,
    _params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    _cache: &mut IdempotencyCache,
    profile: Profile,
) -> Result<Value, DomainError> {
    let caps: Vec<String> = osiris_policy::capabilities(profile)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let tools: Vec<Value> = TOOLS
        .iter()
        .filter(|t| osiris_policy::visible_in(profile, t.name))
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "effect": format!("{:?}", t.effect).to_lowercase(),
            })
        })
        .collect();
    let godot_connected = ipc.is_some();
    let godot_version = ipc.map(|c| c.godot_version().to_string());
    let info = serde_json::json!({
        "profile": format!("{:?}", profile).to_lowercase(),
        "capabilities": caps,
        "godot_connected": godot_connected,
        "godot_version": godot_version,
        "protocol_versions": ["2025-06-18", "2025-03-26", "2024-11-05"],
        "tools": tools,
    });
    Ok(serde_json::json!({
        "content": [{ "type": "text", "text": info.to_string() }],
        "isError": false,
    }))
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

    #[test]
    fn unwrap_tool_result_extracts_payload() {
        let wrapper = serde_json::json!({
            "content": [{ "type": "text", "text": "{\"path\":\"./Btn\",\"name\":\"Btn\"}" }],
            "isError": false,
        });
        let payload = unwrap_tool_result(&wrapper);
        assert_eq!(payload["path"], "./Btn");
    }

    #[test]
    fn resolve_ref_replaces_step_field() {
        let mut results = std::collections::HashMap::new();
        results.insert("btn".to_string(), serde_json::json!({"path": "./Btn"}));
        let resolved = resolve_ref(&serde_json::json!({"path": "$btn.path"}), &results);
        assert_eq!(resolved["path"], "./Btn");
        let missing = resolve_ref(&serde_json::json!({"path": "$btn.nope"}), &results);
        assert_eq!(
            missing["path"], "$btn.nope",
            "campo inexistente se deja como está"
        );
    }
}
