//! Registro de herramientas MCP expuestas por el servidor.
//!
//! El despacho delega a `osiris_application::handle_tool` (ADR-003). Este módulo
//! solo declara la superficie MCP: nombres, descripciones y esquemas.
//! La autorización por capacidades se aplica en `osiris-policy` (ADR-005).

use serde_json::{Value, json};

use crate::jsonrpc::RpcError;
use osiris_application;
use osiris_application::IdempotencyCache;
use osiris_domain::GodotPort;
use osiris_policy::Profile;

/// `tools/call` con nombre desconocido o argumentos inválidos.
pub const INVALID_PARAMS: i64 = -32602;

/// Lista de herramientas para `tools/list`, filtradas por el perfil de capacidades.
#[must_use]
pub fn list(profile: Profile) -> Value {
    let all = all_tools();
    let visible: Vec<Value> = all
        .into_iter()
        .filter(|t| osiris_policy::visible_in(profile, t["name"].as_str().unwrap_or("")))
        .map(with_metadata)
        .collect();
    json!({ "tools": visible })
}

/// Añade metadatos compartidos (RF-011): versión y permiso de la herramienta.
fn with_metadata(mut tool: Value) -> Value {
    let name = tool["name"].as_str().unwrap_or("");
    let permissions = osiris_policy::required(name).map_or("read", |_| "write");
    tool["version"] = json!(env!("CARGO_PKG_VERSION"));
    tool["permissions"] = json!(permissions);
    tool
}

#[allow(clippy::too_many_lines)]
fn all_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "osiris_diagnose",
            "title": "Diagnóstico del servidor",
            "description": "Devuelve nombre, versión y versiones de protocolo soportadas.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot_diagnose",
            "title": "Diagnóstico Godot",
            "description": "Devuelve la versión de Godot y el nombre del proyecto.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.active",
            "title": "Escena activa",
            "description": "Ruta, nombre raíz y número de nodos de la escena activa en el editor.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.nodes",
            "title": "Nodos de la escena",
            "description": "Lista nodos con profundidad y paginación.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath relativo." },
                    "depth": { "type": "integer", "description": "Profundidad máxima." },
                    "max_nodes": { "type": "integer", "description": "Máximo de nodos." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.properties",
            "title": "Propiedad de nodo",
            "description": "Lee una propiedad de un nodo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." },
                    "property": { "type": "string", "description": "Nombre de la propiedad." }
                },
                "required": ["property"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.resources",
            "title": "Recursos de nodo",
            "description": "Lista los recursos referenciados por un nodo.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "additionalProperties": false }
        }),
        json!({
            "name": "godot.editor.errors",
            "title": "Errores del editor",
            "description": "Lista errores y advertencias. Godot 4 no expone API pública.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.create_node",
            "title": "Crear nodo",
            "description": "Crea un nodo en la escena activa. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "type": {"type":"string"}, "name": {"type":"string"} }, "required": ["path","type","name"], "additionalProperties": false }
        }),
        json!({
            "name": "scene.apply",
            "title": "Aplicar operaciones de escena",
            "description": "Ejecuta una lista de operaciones de escena (create_node, remove_node, move_node, set_property, instance_scene) en orden, cada una con Undo/Redo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operations": {
                        "type": "array",
                        "description": "Operaciones a ejecutar en orden."
                    }
                },
                "required": ["operations"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.remove_node",
            "title": "Eliminar nodo",
            "description": "Elimina un nodo con Undo/Redo. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"} }, "required": ["path"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.set_property",
            "title": "Cambiar propiedad",
            "description": "Cambia una propiedad de un nodo con Undo/Redo. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "property": {"type":"string"}, "value": {} }, "required": ["path","property","value"], "additionalProperties": false }
        }),
        json!({
            "name": "object.call",
            "title": "Llamar método",
            "description": "Invoca un método de un nodo con argumentos. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo." },
                    "method": { "type": "string", "description": "Nombre del método." },
                    "args": { "type": "array", "description": "Argumentos (convertidos a Variant)." }
                },
                "required": ["path", "method"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "object.property.get",
            "title": "Leer propiedad de objeto",
            "description": "Lee una propiedad de un nodo (path) o recurso (object res://).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo." },
                    "object": { "type": "string", "description": "Ruta res:// del recurso." },
                    "property": { "type": "string", "description": "Propiedad." }
                },
                "required": ["property"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "object.property.set",
            "title": "Escribir propiedad de objeto",
            "description": "Pone una propiedad de un nodo (con Undo/Redo) o recurso. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo." },
                    "object": { "type": "string", "description": "Ruta res:// del recurso." },
                    "property": { "type": "string", "description": "Propiedad." },
                    "value": { "description": "Valor." }
                },
                "required": ["property", "value"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.save",
            "title": "Guardar escena",
            "description": "Guarda la escena activa. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.run",
            "title": "Ejecutar proyecto",
            "description": "Inicia la ejecución del proyecto o de una escena específica. Requiere perfil admin.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scene": { "type": "string", "description": "Ruta de la escena opcional a ejecutar (ej: res://main.tscn)." },
                    "timeout_secs": { "type": "integer", "description": "Tiempo máximo opcional de ejecución antes de auto-detención." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.stop",
            "title": "Detener proyecto",
            "description": "Detiene la ejecución del proyecto. Requiere perfil admin.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.run.status",
            "title": "Estado de ejecución",
            "description": "Devuelve si el proyecto está en ejecución. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.run.output",
            "title": "Salida de ejecución",
            "description": "Devuelve las últimas líneas de salida (log) del proyecto ejecutado. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.assert.property",
            "title": "Verificar propiedad",
            "description": "Compara el valor de una propiedad con un esperado. Devuelve match (bool) y el valor real.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." },
                    "property": { "type": "string", "description": "Nombre de la propiedad." },
                    "expected": { "description": "Valor esperado." }
                },
                "required": ["property"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.script.validate",
            "title": "Validar sintaxis de script",
            "description": "Valida la sintaxis de un código GDScript antes de escribirlo en disco.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "string", "description": "Código GDScript a validar." }
                },
                "required": ["code"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.script.write",
            "title": "Escribir script",
            "description": "Valida y escribe un script GDScript a res://*.gd en disco. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ruta res:// del script (ej: res://player.gd)." },
                    "code": { "type": "string", "description": "Código GDScript completo." }
                },
                "required": ["path", "code"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.describe",
            "title": "Describir clase Godot",
            "description": "Reflexión del ClassDB (ADR-004): herencia, propiedades, métodos, señales y constantes de una clase Godot. Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "class": { "type": "string", "description": "Nombre de la clase (ej: Node2D, CharacterBody2D)." },
                    "recursive": { "type": "boolean", "description": "Incluir miembros heredados (default false)." },
                    "max_props": { "type": "integer", "description": "Límite de propiedades devueltas (default 300)." },
                    "max_methods": { "type": "integer", "description": "Límite de métodos devueltos (default 300)." }
                },
                "required": ["class"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.signal.list",
            "title": "Señales de un nodo",
            "description": "Lista las señales de un nodo con sus argumentos.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.signal.connect",
            "title": "Conectar señal",
            "description": "Conecta la señal de un nodo a un método de otro nodo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo fuente." },
                    "signal": { "type": "string", "description": "Nombre de la señal." },
                    "target_path": { "type": "string", "description": "NodePath del nodo destino." },
                    "method": { "type": "string", "description": "Método a llamar." },
                    "deferred": { "type": "boolean", "description": "Conectar diferido (CONNECT_DEFERRED)." },
                    "oneshot": { "type": "boolean", "description": "Conectar solo una vez (CONNECT_ONESHOT)." }
                },
                "required": ["path", "signal", "target_path", "method"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.signal.disconnect",
            "title": "Desconectar señal",
            "description": "Desconecta la señal de un nodo de un método. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo fuente." },
                    "signal": { "type": "string", "description": "Nombre de la señal." },
                    "target_path": { "type": "string", "description": "NodePath del nodo destino." },
                    "method": { "type": "string", "description": "Método a desconectar." }
                },
                "required": ["path", "signal", "target_path", "method"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.asset.list",
            "title": "Listar assets",
            "description": "Enumera archivos del proyecto (recursivo) con tipo y estado de importación.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directorio res:// (default res://)." },
                    "max_total": { "type": "integer", "description": "Máximo de archivos (default 500)." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.project.files",
            "title": "Archivos del proyecto",
            "description": "Lista archivos fuente del proyecto (DirAccess, excluye .godot/ y addons/) con tamaño. Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "max_total": { "type": "integer", "description": "Máximo de archivos (default 500)." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.project.settings.get",
            "title": "Leer settings del proyecto",
            "description": "Lee settings de project.godot (application/config/name, display/window/...).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Una setting (ej: application/config/name)." },
                    "names": { "type": "array", "description": "Varias settings." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.project.settings.set",
            "title": "Escribir settings del proyecto",
            "description": "Pone una setting y guarda project.godot. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Setting (ej: application/config/name)." },
                    "value": { "description": "Valor." }
                },
                "required": ["name", "value"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.asset.reimport",
            "title": "Reimportar asset",
            "description": "Fuerza la reimportación de un archivo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ruta res:// a reimportar." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.resource.assign",
            "title": "Asignar recurso",
            "description": "Carga un recurso res:// y lo asigna a una propiedad de un nodo con Undo/Redo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo." },
                    "property": { "type": "string", "description": "Propiedad a asignar." },
                    "resource": { "type": "string", "description": "Ruta res:// del recurso (ej: res://icon.svg)." }
                },
                "required": ["path", "property", "resource"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "plan.validate",
            "title": "Validar plan",
            "description": "Valida la estructura de un plan (steps con tool conocida, params objeto) sin ejecutar nada.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "plan": {
                        "type": "object",
                        "description": "Plan: { steps: [{tool, id?, params?}] }."
                    }
                },
                "required": ["plan"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "plan.execute",
            "title": "Ejecutar plan",
            "description": "Ejecuta los pasos de un plan en orden. Los params de un paso pueden referenciar el resultado de uno previo con $id.campo. Un paso fallido aborta el plan. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "plan": {
                        "type": "object",
                        "description": "Plan: { steps: [{tool, id?, params?}] }."
                    }
                },
                "required": ["plan"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "skill.list",
            "title": "Listar skills",
            "description": "Lista las skills disponibles (conocimiento declarativo para resolver intenciones).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "skill.get",
            "title": "Obtener skill",
            "description": "Devuelve el detalle de una skill: tools que usa y conocimiento de apoyo.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nombre de la skill (ver skill.list)." }
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "osiris.capabilities",
            "title": "Catálogo de capacidades",
            "description": "Devuelve qué capacidades y tools están disponibles para el perfil, Godot y protocolo actuales (RF-12).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.runtime.tree",
            "title": "Árbol remoto del juego",
            "description": "Árbol de nodos del juego en ejecución (runtime) o de la escena activa del editor como fallback. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.runtime.metrics",
            "title": "Métricas de rendimiento",
            "description": "Monitores de rendimiento del juego en ejecución (FPS, memoria, ...). Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "debug.inspect",
            "title": "Inspeccionar objeto en runtime",
            "description": "Inspecciona un objeto del juego en ejecución por id del árbol remoto (o path) vía el debugger. Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Id del objeto (del árbol remoto)." },
                    "path": { "type": "string", "description": "Nombre del nodo (alternativo al id)." }
                },
                "additionalProperties": false
            }
        }),
    ]
}

/// Ejecuta `tools/call`. `ipc` es la conexión al puente, si existe.
///
/// # Errors
/// `RpcError` con `INVALID_PARAMS` si falta `params.name` o la herramienta es
/// desconocida (spec MCP: error de protocolo, no `isError`).
pub fn call(
    params: &Value,
    ipc: Option<&mut dyn GodotPort>,
    profile: Profile,
    cache: &mut IdempotencyCache,
) -> Result<Value, RpcError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::new(INVALID_PARAMS, "params.name requerido"))?;
    match name {
        "osiris_diagnose"
        | "godot_diagnose"
        | "godot.scene.active"
        | "godot.scene.nodes"
        | "godot.scene.properties"
        | "godot.scene.resources"
        | "godot.editor.errors"
        | "godot.scene.create_node"
        | "godot.scene.remove_node"
        | "godot.scene.set_property"
        | "scene.apply"
        | "object.call"
        | "object.property.get"
        | "object.property.set"
        | "godot.scene.save"
        | "godot.run"
        | "godot.stop"
        | "godot.run.status"
        | "godot.run.output"
        | "godot.assert.property"
        | "godot.script.validate"
        | "godot.script.write"
        | "godot.describe"
        | "godot.signal.list"
        | "godot.signal.connect"
        | "godot.signal.disconnect"
        | "godot.asset.list"
        | "godot.project.files"
        | "godot.project.settings.get"
        | "godot.project.settings.set"
        | "godot.asset.reimport"
        | "godot.resource.assign"
        | "plan.validate"
        | "plan.execute"
        | "skill.list"
        | "skill.get"
        | "osiris.capabilities"
        | "godot.runtime.tree"
        | "godot.runtime.metrics"
        | "debug.inspect" => {
            let args = params.get("arguments").unwrap_or(&Value::Null);
            osiris_application::handle_tool(name, args, ipc, profile, cache)
                .map_err(|e| RpcError::from_domain(&e))
        }
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            format!("herramienta desconocida: {name}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tool_names_are_unique() {
        let tools = all_tools();
        let names: HashSet<String> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names.len(), tools.len(), "RF-014: nombres duplicados");
    }

    #[test]
    fn every_tool_has_name_and_schema() {
        for t in all_tools() {
            assert!(t["name"].is_string(), "tool sin name: {t}");
            assert!(
                t["inputSchema"].is_object(),
                "{} sin inputSchema",
                t["name"]
            );
        }
    }

    #[test]
    fn metadata_version_and_permissions_present() {
        let tools = list(Profile::Read)["tools"].as_array().unwrap().clone();
        for t in tools {
            assert_eq!(
                t["version"],
                env!("CARGO_PKG_VERSION"),
                "tool {} sin version",
                t["name"]
            );
            assert!(
                t["permissions"].is_string(),
                "tool {} sin permissions",
                t["name"]
            );
        }
    }
}
