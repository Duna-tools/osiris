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
        name: "osiris.state",
        op: Some("get_state"),
        effect: Effect::Read,
        handler: ipc_plain,
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
        name: "godot.node.tween",
        op: Some("create_tween"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.node.desc",
        op: Some("set_description"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.node.unique",
        op: Some("set_unique_name"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.node.reparent",
        op: Some("reparent_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "scene.apply",
        op: Some("scene_apply"),
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
        name: "object.call",
        op: Some("call_method"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "object.property.get",
        op: Some("object_property_get"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "object.property.set",
        op: Some("object_property_set"),
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
        name: "godot.scene.save_as",
        op: Some("save_scene_as"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.save_all",
        op: Some("save_all_scenes"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.unsaved",
        op: Some("unsaved_scenes"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.open",
        op: Some("open_scene"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.create",
        op: Some("create_scene"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.select",
        op: Some("select_nodes"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.duplicate",
        op: Some("duplicate_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.instance",
        op: Some("instance_scene"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.move",
        op: Some("move_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.inputmap.add",
        op: Some("inputmap_add"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scenes.open",
        op: Some("open_scenes"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.export",
        op: Some("export_project"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.scene.screenshot",
        op: Some("editor_screenshot"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.project.autoload",
        op: Some("add_autoload"),
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
        name: "godot.input.send",
        op: Some("input_send"),
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
        name: "godot.editor.reload",
        op: Some("reload_editor"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.editor.inspect",
        op: Some("inspect_node"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.editor.settings.get",
        op: Some("editor_settings_get"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.editor.settings.set",
        op: Some("editor_settings_set"),
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
        name: "osiris.run.history",
        op: Some("run_history"),
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
        name: "godot.script.write",
        op: Some("write_script"),
        effect: Effect::Write,
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
        name: "godot.signal.emit",
        op: Some("emit_signal"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.group.add",
        op: Some("add_to_group"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.group.remove",
        op: Some("remove_from_group"),
        effect: Effect::Write,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.group.list",
        op: Some("get_groups"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.tree.nodes_in_group",
        op: Some("nodes_in_group"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.input.query",
        op: Some("input_query"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.asset.list",
        op: Some("asset_list"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.project.files",
        op: Some("project_files"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.project.settings.get",
        op: Some("project_settings_get"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "godot.project.settings.set",
        op: Some("project_settings_set"),
        effect: Effect::Write,
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
        name: "godot.resource.create",
        op: Some("resource_create"),
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
    ToolSpec {
        name: "debug.inspect",
        op: Some("debug_inspect"),
        effect: Effect::Read,
        handler: ipc_plain,
    },
    ToolSpec {
        name: "runtime.capture",
        op: Some("runtime_capture"),
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
        description: "Crea un player 2D básico (CharacterBody2D con Sprite2D y CollisionShape2D) en la escena activa, incluyendo input actions.",
        uses: &[
            "godot.describe",
            "godot.scene.create_node",
            "godot.scene.set_property",
            "godot.scene.duplicate",
            "godot.resource.assign",
            "godot.resource.create",
            "godot.script.validate",
            "godot.script.write",
            "godot.signal.connect",
            "godot.inputmap.add",
        ],
        knowledge: &[
            "CharacterBody2D es la base de movimiento para jugadores 2D; movimiento usa velocity + move_and_slide() en _physics_process",
            "Crea las acciones de input ANTES de escribir el script: usa godot.inputmap.add para 'move_left', 'move_right', 'jump'",
            "El player necesita CollisionShape2D con un RectangleShape2D o CircleShape2D como hijo",
            "Los scripts se validan con godot.script.validate antes de escribir con godot.script.write",
            "Para sprites animados usa AnimatedSprite2D + SpriteFrames (godot.resource.create + godot.resource.assign)",
        ],
    },
    Skill {
        name: "create_ui_button",
        description: "Crea un botón UI con señal pressed conectada a un método del nodo raíz.",
        uses: &[
            "godot.describe",
            "godot.scene.create_node",
            "godot.scene.set_property",
            "godot.signal.connect",
            "godot.signal.emit",
        ],
        knowledge: &[
            "Button extiende BaseButton; la señal pressed se emite al hacer clic",
            "El método destino debe existir en el nodo receptor; usa godot.script.validate + godot.script.write para crearlo",
            "godot.signal.emit permite disparar la señal manualmente para probar",
        ],
    },
    Skill {
        name: "create_scene",
        description: "Crea una escena nueva (res://*.tscn) con una raíz del tipo especificado y la estructura básica.",
        uses: &[
            "godot.scene.create",
            "godot.scene.open",
            "godot.scene.create_node",
            "godot.scene.set_property",
            "godot.scene.save",
            "godot.scenes.open",
        ],
        knowledge: &[
            "godot.scene.create crea y abre una escena vacía con el nodo raíz dado",
            "Tras crear la escena, usa godot.scene.create_node para añadir hijos",
            "godot.scene.open abre una escena existente; usa godot.scenes.open para listar las abiertas",
            "Guarda con godot.scene.save antes de cambiar a otra escena",
        ],
    },
    Skill {
        name: "setup_input",
        description: "Configura las acciones de input del proyecto (InputMap) para gameplay.",
        uses: &["godot.inputmap.add", "godot.project.autoload"],
        knowledge: &[
            "godot.inputmap.add crea una acción de InputMap y la persiste en project.godot",
            "Acciones comunes 2D: ui_left, ui_right, ui_up, ui_down, jump, attack",
            "Acciones del demo truck-town: accelerate, reverse, turn_left, turn_right, boost, honk",
            "Para input avanzado (teclas personalizadas), usa godot.project.settings.set con 'input/<accion>'",
            "El autoload OsirisInput permite al LLM enviar input al juego en ejecución (godot.input.send)",
        ],
    },
    Skill {
        name: "create_sprite_animation",
        description: "Crea SpriteFrames con animaciones para un AnimatedSprite2D.",
        uses: &[
            "godot.resource.create",
            "godot.resource.assign",
            "godot.scene.set_property",
            "godot.describe",
        ],
        knowledge: &[
            "godot.resource.create con type='SpriteFrames' crea los frames vacíos; luego usa object.call con object='res://...' para add_animation y add_frame",
            "Asigna los SpriteFrames a un AnimatedSprite2D con godot.resource.assign en la propiedad 'sprite_frames'",
            "object.call acepta object (res://) para llamar métodos en recursos: add_animation('walk'), add_frame('walk', texture, 0.1)",
            "Para textures usa godot.asset.list para ver assets disponibles y object.call con $res para pasarlos",
        ],
    },
    Skill {
        name: "game_loop",
        description: "Máquina de estados del desarrollo: qué transición hacer en cada estado. Consulta osiris.state antes de cada paso.",
        uses: &[
            "osiris.state",
            "godot.scene.save",
            "godot.run",
            "godot.input.send",
            "godot.runtime.tree",
            "godot.scene.screenshot",
            "godot.editor.errors",
            "godot.script.validate",
            "godot.script.write",
            "godot.stop",
            "godot.export",
            "godot.resource.create",
        ],
        knowledge: &[
            "El desarrollo es un DFA: consulta osiris.state, lee en qué estado estás y aplica la única transición válida.",
            "ESTADO edit -> RUN: modifica la escena (scene.create/apply/set_property/duplicate/instance), guarda (scene.save), luego godot.run.",
            "ESTADO run -> observe: tras godot.run, verifica osiris.state.debugger_session=connected; si es pending, reintenta en ~2s.",
            "ESTADO observe -> act: runtime.tree para ver el árbol, input.send para enviar input (accelerate/turn_left/turn_right/reverse/boost/honk en truck-town).",
            "ESTADO observe -> fix: si editor.errors tiene errores, corrige con script.validate + script.write y vuelve a RUN. Crea sprites con resource.create.",
            "ESTADO observe -> done: si el objetivo se cumplió, godot.stop y export si corresponde.",
            "Siempre consulta osiris.state antes de una tool de transición: evita llamar godot.run si game.running=true, o input.send si debugger_session != connected.",
        ],
    },
    Skill {
        name: "setup_physics",
        description: "Configura collision layers y shapes para física 2D/3D en Godot.",
        uses: &[
            "godot.resource.create",
            "godot.resource.assign",
            "godot.scene.set_property",
            "godot.scene.create_node",
            "object.call",
        ],
        knowledge: &[
            "Colisiones 2D: crea un CollisionShape2D como hijo del cuerpo, crea un RectangleShape2D o CircleShape2D con resource.create, ajusta size con object.call, asigna con resource.assign",
            "Ejemplo: resource.create(type='RectangleShape2D', path='res://shapes/player_shape.tres') → object.call(object='res://shapes/player_shape.tres', method='set_size', args=[{'x':64,'y':64}]) → resource.assign(path='Player', property='shape', resource='res://...')",
            "Collision layers: scene.set_property(path='Player', property='collision_layer', value=1) y collision_mask=1 para colisionar con el layer 1",
            "Para StaticBody2D/RigidBody2D: scene.set_property para collision_layer/collision_mask (bitmask int)",
            "Area2D usa body_entered/area_entered signals (godot.signal.connect) para detectar colisiones",
        ],
    },
    Skill {
        name: "setup_navigation",
        description: "Configura navegación 2D con NavigationRegion2D y NavigationAgent2D para pathfinding.",
        uses: &[
            "godot.resource.create",
            "godot.resource.assign",
            "godot.scene.create_node",
            "godot.scene.set_property",
            "object.call",
            "godot.signal.connect",
        ],
        knowledge: &[
            "Crea NavigationRegion2D como hijo de la escena. Crea NavigationPolygon con resource.create(type='NavigationPolygon'). Usa object.call para añadir vertices y polígonos",
            "NavigationAgent2D va en el personaje (CharacterBody2D): crealo como hijo, conecta signal 'navigation_finished'",
            "En el script del personaje: navigation_agent.target_position = destino; luego en _physics_process: var dir = (navigation_agent.get_next_path_position() - global_position).normalized(); velocity = dir * speed",
            "La navegación requiere un NavigationRegion2D baked (con polígono) y agentes configurados con velocidad y radio",
        ],
    },
    Skill {
        name: "setup_animation",
        description: "Crea animaciones con AnimationPlayer y AnimationLibrary.",
        uses: &[
            "godot.scene.create_node",
            "godot.resource.create",
            "godot.resource.assign",
            "object.call",
            "godot.scene.set_property",
        ],
        knowledge: &[
            "Crea AnimationPlayer como hijo del nodo a animar. Crea Animation resource: godot.resource.create(type='Animation', path='res://animations/player.tres')",
            "Añade la animación al player: object.call sobre el AnimationPlayer para acceder a animation_library, o usa object.call(object='res://...', method='add_track', args=[TYPE_VALUE, 0]) donde TYPE_VALUE=0",
            "Añade keyframes con object.call: track_insert_key(track_idx, time, value). Para posición: track_set_key_value(track_idx, key_idx, Vector2(x,y))",
            "Tipos de track: 0=value, 1=position (3D), 2=rotation (3D), 3=scale (3D). Para 2D usa value con property 'position'",
            "Reproduce con object.call: play('anim_name') y deten con stop()",
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

/// Tools de estado vivo cuyas lecturas NO deben cachearse: datos volátiles que
/// cambian entre llamadas con los mismos parámetros (ej: run.status, runtime.tree,
/// editor.errors). Cachearlas devuelve datos obsoletos al LLM.
const UNCACHEABLE: &[&str] = &[
    "godot.editor.errors",
    "godot.run.status",
    "godot.run.output",
    "godot.runtime.tree",
    "godot.runtime.metrics",
    "debug.inspect",
    "runtime.capture",
    "godot.scene.screenshot",
    "osiris.state",
];

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

    if spec.effect == Effect::Write || UNCACHEABLE.contains(&name) {
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
