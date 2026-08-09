@tool
extends EditorPlugin

const BIND_ADDR := "127.0.0.1"
const BIND_PORT := 9876

var _server: RefCounted = null
var _debugger_node: Node = null
var _connected_sessions: Dictionary = {}
var _controller: EditorDebuggerPlugin = null
var _requested_sessions: Dictionary = {}

func _enter_tree() -> void:
	@warning_ignore("inferred_declaration")
	randomize()
	_server = preload("res://addons/osiris-bridge/server.gd").new()
	_server.start(BIND_ADDR, BIND_PORT, get_editor_interface(), get_undo_redo())
	_controller = preload("res://addons/osiris-bridge/debugger_controller.gd").new()
	add_debugger_plugin(_controller)
	if FileAccess.file_exists("res://scenes/main.tscn"):
		get_editor_interface().open_scene_from_path("res://scenes/main.tscn")

func _exit_tree() -> void:
	if _server:
		_server.stop()
		_server = null
	_controller = null
	_connected_sessions.clear()

func _process(_delta: float) -> void:
	if _server:
		_server.poll()
	_attach_debugger()
	_refresh_runtime()

var _last_refresh: int = 0

## Pide árbol y métricas periódicamente mientras el juego corre (el primer
## request puede perderse antes de que la sesión esté activa).
func _refresh_runtime() -> void:
	if _controller == null or _server == null:
		return
	if not _server.is_running():
		_requested_sessions.clear()
		return
	if Time.get_ticks_msec() - _last_refresh < 2000:
		return
	_last_refresh = Time.get_ticks_msec()
	_controller.request_tree()
	_controller.enable_metrics()

## Conecta la señal debug_data de cada sesión del debugger del editor. La señal
## se emite para TODOS los mensajes del juego (vault 18 §7); error/output tienen
## handlers nativos, así que el plugin EditorDebuggerPlugin no los recibe, pero
## debug_data sí los ve siempre.
func _attach_debugger() -> void:
	if _debugger_node == null or not is_instance_valid(_debugger_node):
		_debugger_node = _find_debugger_node()
		_connected_sessions.clear()
	if _debugger_node == null:
		return
	var sessions: Array = _debugger_node.find_children("*", "ScriptEditorDebugger", true, false)
	for child: Node in sessions:
		if _connected_sessions.has(child.get_instance_id()):
			continue
		_connected_sessions[child.get_instance_id()] = true
		child.connect("debug_data", Callable(_server, "_on_debug_data"))
		if _controller and not _requested_sessions.has(child.get_instance_id()):
			_requested_sessions[child.get_instance_id()] = true
			# Pide el árbol remoto y activa métricas una vez por sesión.
			_controller.request_tree()
			_controller.enable_metrics()

## El dock "Debugger" tiene nombre localizado (TTRC); buscarlo por clase es
## estable entre idiomas.
func _find_debugger_node() -> Node:
	var base: Control = get_editor_interface().get_base_control()
	if base == null:
		return null
	var by_class: Array = base.find_children("*", "EditorDebuggerNode", true, false)
	if by_class.size() > 0:
		return by_class[0]
	return null
