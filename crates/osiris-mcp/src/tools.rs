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
            "name": "osiris.state",
            "title": "Estado actual",
            "description": "Estado completo de la máquina (editor + juego + sesión debugger + input). Consulta esto ANTES de elegir la siguiente tool: cada estado tiene una transición válida.",
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
            "name": "godot.node.tween",
            "title": "Crear Tween",
            "description": "Crea un Tween en un nodo para animaciones. Usa object.call sobre el Tween devuelto. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"} }, "required": ["path"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.node.desc",
            "title": "Descripcion de nodo",
            "description": "Asigna una descripcion en el editor al nodo (visible en el inspector). Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "description": {"type":"string"} }, "required": ["path","description"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.node.unique",
            "title": "Acceso unico",
            "description": "Marca/desmarca un nodo con acceso % (set_unique_name_in_owner). Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "enable": {"type":"boolean"} }, "required": ["path"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.node.reparent",
            "title": "Redpadrear nodo",
            "description": "Cambia el padre de un nodo preservando la transformacion global (Node.reparent). Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": { "path": {"type":"string"}, "new_parent": {"type":"string"}, "keep_transform": {"type":"boolean"} }, "required": ["path","new_parent"], "additionalProperties": false }
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
            "description": "Invoca un método de un nodo (path) o recurso (object res://). Args soportan {\"$res\": \"res://\"} para pasar recursos. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath (para nodos) o usar object (para recursos)." },
                    "object": { "type": "string", "description": "Ruta res:// del recurso (alternativa a path)." },
                    "method": { "type": "string", "description": "Nombre del método." },
                    "args": { "type": "array", "description": "Argumentos. Usa {\"$res\":\"res://\"} para recursos." }
                },
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
            "name": "godot.scene.save_as",
            "title": "Guardar escena como",
            "description": "Guarda la escena activa con un nuevo path (res://*.tscn). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Nueva ruta res:// de la escena." } },
                "required": ["path"], "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.save_all",
            "title": "Guardar todas",
            "description": "Guarda todas las escenas abiertas. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.unsaved",
            "title": "Escenas sin guardar",
            "description": "Lista las escenas con cambios sin guardar. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scenes.open",
            "title": "Escenas abiertas",
            "description": "Lista las escenas abiertas en el editor. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.scene.open",
            "title": "Abrir escena",
            "description": "Abre una escena (res://*.tscn) en el editor. Fallo si hay escenas sin guardar: guarda primero (godot.scene.save). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ruta res:// de la escena a abrir (ej: res://scenes/menu.tscn)." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.create",
            "title": "Crear escena",
            "description": "Crea una escena nueva (res://*.tscn) con una raíz del tipo dado y la deja abierta en el editor. Fallo si hay escenas sin guardar. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ruta res:// de la nueva escena (ej: res://scenes/menu.tscn)." },
                    "type": { "type": "string", "description": "Tipo del nodo raíz (default Node2D)." },
                    "name": { "type": "string", "description": "Nombre del nodo raíz (default: el tipo)." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.duplicate",
            "title": "Duplicar nodo",
            "description": "Duplica un nodo (duplicado profundo) como hermano, con Undo/Redo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo a duplicar." },
                    "name": { "type": "string", "description": "Nombre del duplicado (default: nombre original)." }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.instance",
            "title": "Instanciar escena",
            "description": "Instancia una escena empaquetada (res://*.tscn) como hijo de un nodo, con Undo/Redo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo padre." },
                    "scene": { "type": "string", "description": "Ruta res:// de la escena a instanciar." },
                    "name": { "type": "string", "description": "Nombre del nodo instanciado (default: el de la escena)." }
                },
                "required": ["path", "scene"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.move",
            "title": "Mover nodo",
            "description": "Mueve un nodo a un nuevo padre con Undo/Redo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo a mover." },
                    "new_parent": { "type": "string", "description": "NodePath del nuevo padre." },
                    "new_name": { "type": "string", "description": "Nuevo nombre opcional." }
                },
                "required": ["path", "new_parent"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.inputmap.add",
            "title": "Registrar acción de input",
            "description": "Crea una acción de InputMap y la persiste en project.godot. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "Nombre de la acción (ej: ui_left, jump)." }
                },
                "required": ["action"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.select",
            "title": "Seleccionar nodos",
            "description": "Selecciona nodos (si pasas paths) o lee la selección actual (sin paths). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": { "type": "array", "items": { "type": "string" }, "description": "NodePaths a seleccionar. Sin paths = solo leer selección actual." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.scene.screenshot",
            "title": "Screenshot del editor",
            "description": "Captura el viewport del editor (2D o 3D) a un PNG. Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view3d": { "type": "boolean", "description": "true = viewport 3D, false (default) = viewport 2D." },
                    "output": { "type": "string", "description": "Ruta de salida user:// o res:// (default user://osiris_editor_shot.png)." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.project.autoload",
            "title": "Registrar autoload",
            "description": "Registra un autoload singleton en project.godot (se instancia al siguiente arranque del proyecto). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nombre del singleton (ej: GameManager)." },
                    "path": { "type": "string", "description": "Ruta res:// del script (ej: res://game_manager.gd)." }
                },
                "required": ["name", "path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.export",
            "title": "Exportar proyecto",
            "description": "Exporta el proyecto a un preset de export_presets.cfg (release o debug). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "preset": { "type": "string", "description": "Nombre del preset definido en export_presets.cfg." },
                    "output": { "type": "string", "description": "Ruta de salida (ej: build/game.exe o build/game.pck)." },
                    "debug": { "type": "boolean", "description": "true = --export-debug, false (default) = --export-release." }
                },
                "required": ["preset", "output"],
                "additionalProperties": false
            }
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
            "name": "godot.input.send",
            "title": "Enviar input al juego",
            "description": "Inyecta un input (tecla o acción) en el juego en ejecución vía Live Edit. Requiere perfil admin.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "input": {
                        "type": "object",
                        "description": "Dict: {\"key\": \"W\", \"pressed\": true} o {\"action\": \"jump\", \"pressed\": true}.",
                        "properties": {
                            "key": { "type": "string", "description": "Nombre de la tecla (ej: W, Space, ui_left)." },
                            "action": { "type": "string", "description": "Acción de InputMap (ej: jump)." },
                            "pressed": { "type": "boolean", "description": "true = pulsada, false = soltada." }
                        }
                    }
                },
                "required": ["input"],
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
            "name": "godot.editor.reload",
            "title": "Recargar editor",
            "description": "Recarga silenciosamente las escenas abiertas desde disco (evita el dialogo 'files changed, reload?'). Usalo tras modificar archivos externamente. Requiere perfil edit.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "godot.editor.inspect",
            "title": "Inspeccionar nodo",
            "description": "Abre el inspector del editor en un nodo (path) o lo limpia (sin path). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "NodePath. Sin path = limpiar inspector." } },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.editor.settings.get",
            "title": "Leer config del editor",
            "description": "Lee una setting del Editor (EditorSettings). Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "Nombre de la setting (ej: interface/theme/base_color)." } },
                "required": ["name"], "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.editor.settings.set",
            "title": "Escribir config del editor",
            "description": "Cambia una setting del Editor (EditorSettings). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "value": { "description": "Nuevo valor." }
                },
                "required": ["name", "value"], "additionalProperties": false
            }
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
            "name": "osiris.run.history",
            "title": "Historial de ejecuciones",
            "description": "Historial persistido de ejecuciones del proyecto (escena, duración, salida, errores). Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "max_entries": { "type": "integer", "description": "Máximo de entradas (default 50)." },
                    "scene": { "type": "string", "description": "Filtrar por escena (ej: res://main.tscn)." }
                },
                "additionalProperties": false
            }
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
            "name": "godot.signal.emit",
            "title": "Emitir señal",
            "description": "Emite una señal en un nodo con argumentos opcionales. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath del nodo." },
                    "signal": { "type": "string", "description": "Nombre de la señal." },
                    "args": { "type": "array", "description": "Argumentos de la señal." }
                },
                "required": ["path", "signal"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.group.add",
            "title": "Añadir a grupo",
            "description": "Añade un nodo a un grupo (Node.add_to_group). Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "NodePath." },
                    "group": { "type": "string", "description": "Nombre del grupo." }
                },
                "required": ["path", "group"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.group.remove",
            "title": "Quitar de grupo",
            "description": "Quita un nodo de un grupo. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "group": { "type": "string" }
                },
                "required": ["path", "group"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.group.list",
            "title": "Listar grupos",
            "description": "Lista los grupos a los que pertenece un nodo. Solo lectura.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "godot.tree.nodes_in_group",
            "title": "Nodos en grupo",
            "description": "Devuelve todos los nodos de la escena activa que pertenecen a un grupo. Solo lectura.",
            "inputSchema": { "type": "object", "properties": { "group": {"type":"string"} }, "required": ["group"], "additionalProperties": false }
        }),
        json!({
            "name": "godot.input.query",
            "title": "Consultar input",
            "description": "Lee el estado de input (is_action_pressed, get_vector, is_key_pressed). Solo lectura.",
            "inputSchema": { "type": "object", "properties": { "actions": {"type":"array"}, "keys": {"type":"array"}, "vector": {"type":"string"} }, "additionalProperties": false }
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
            "name": "godot.resource.create",
            "title": "Crear recurso",
            "description": "Crea un recurso nuevo (Animation, Theme, SpriteFrames, ShaderMaterial...) y lo guarda a res://*.tres. Requiere perfil edit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "Clase del recurso (ej: Animation, Theme, SpriteFrames)." },
                    "path": { "type": "string", "description": "Ruta res:// de salida (ej: res://assets/player_anim.tres)." },
                    "properties": { "type": "object", "description": "Propiedades iniciales (opcional), dict nombre -> valor." }
                },
                "required": ["type", "path"],
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
        json!({
            "name": "runtime.capture",
            "title": "Capturar screenshot del juego",
            "description": "Pide al juego en ejecución un screenshot; devuelve la ruta del PNG guardado en temp. Solo lectura.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
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
        | "osiris.state"
        | "godot_diagnose"
        | "godot.scene.active"
        | "godot.scene.nodes"
        | "godot.scene.properties"
        | "godot.scene.resources"
        | "godot.editor.errors"
        | "godot.scene.create_node"
        | "godot.node.tween"
        | "godot.node.desc"
        | "godot.node.unique"
        | "godot.node.reparent"
        | "godot.scene.remove_node"
        | "godot.scene.set_property"
        | "scene.apply"
        | "object.call"
        | "object.property.get"
        | "object.property.set"
        | "godot.scene.save"
        | "godot.scene.save_as"
        | "godot.scene.save_all"
        | "godot.scene.unsaved"
        | "godot.scene.open"
        | "godot.scene.create"
        | "godot.scene.select"
        | "godot.scene.duplicate"
        | "godot.scene.instance"
        | "godot.scene.move"
        | "godot.inputmap.add"
        | "godot.scene.screenshot"
        | "godot.project.autoload"
        | "godot.scenes.open"
        | "godot.export"
        | "godot.run"
        | "godot.input.send"
        | "godot.stop"
        | "godot.editor.reload"
        | "godot.editor.inspect"
        | "godot.editor.settings.get"
        | "godot.editor.settings.set"
        | "godot.run.status"
        | "godot.run.output"
        | "osiris.run.history"
        | "godot.assert.property"
        | "godot.script.validate"
        | "godot.script.write"
        | "godot.describe"
        | "godot.signal.list"
        | "godot.signal.connect"
        | "godot.signal.disconnect"
        | "godot.signal.emit"
        | "godot.group.add"
        | "godot.group.remove"
        | "godot.group.list"
        | "godot.tree.nodes_in_group"
        | "godot.input.query"
        | "godot.asset.list"
        | "godot.project.files"
        | "godot.project.settings.get"
        | "godot.project.settings.set"
        | "godot.asset.reimport"
        | "godot.resource.assign"
        | "godot.resource.create"
        | "plan.validate"
        | "plan.execute"
        | "skill.list"
        | "skill.get"
        | "osiris.capabilities"
        | "godot.runtime.tree"
        | "godot.runtime.metrics"
        | "debug.inspect"
        | "runtime.capture" => {
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
