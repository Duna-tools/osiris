@tool
class_name OsirisDebuggerController
extends EditorDebuggerPlugin

## Controla el debugger del editor: pide el árbol remoto y métricas del juego.
## El plugin EditorDebuggerPlugin._capture NO recibe error/output (handlers
## nativos), pero sus sesiones SÍ exponen send_message() para pedir datos.
## Ver vault 18_errores_godot.md §7.

var _session: Object = null
var _plugin: Object = null

func set_plugin(p: Object) -> void:
	_plugin = p

func _setup_session(session_id: int) -> void:
	_session = get_session(session_id)
	# Avisar al plugin: conecta debug_data a la sesión recién creada. El
	# session_id es el índice del tab (editor_debugger_plugin.cpp:157), que
	# coincide con el orden de los ScriptEditorDebugger bajo el dock.
	if _plugin and _plugin.has_method("_on_session_created"):
		var node: Node = _find_session_node(session_id)
		if node:
			_plugin.call("_on_session_created", node)

func _find_session_node(idx: int) -> Node:
	var plugin: EditorPlugin = _plugin
	var base: Control = plugin.get_editor_interface().get_base_control()
	if base == null:
		return null
	var sessions: Array = base.find_children("*", "ScriptEditorDebugger", true, false)
	if idx >= 0 and idx < sessions.size():
		return sessions[idx]
	return null

func request_tree() -> void:
	if _session:
		_session.send_message("scene:request_scene_tree", [])

func request_inspect(ids: Array) -> void:
	if _session:
		_session.send_message("scene:inspect_objects", [ids, false])

var _screenshot_id: int = 0

func request_screenshot() -> int:
	_screenshot_id += 1
	if _session:
		_session.send_message("scene:rq_screenshot", [_screenshot_id])
	return _screenshot_id

func enable_metrics() -> void:
	if _session:
		_session.toggle_profiler("performance", true, [])
