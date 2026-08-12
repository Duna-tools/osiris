@tool
extends EditorPlugin

const BIND_ADDR := "127.0.0.1"
const BIND_PORT := 9876

var _server: RefCounted = null
var _connected_sessions: Dictionary = {}
var _controller: EditorDebuggerPlugin = null
var _requested_sessions: Dictionary = {}

func _enter_tree() -> void:
	@warning_ignore("inferred_declaration")
	randomize()
	_server = preload("res://addons/osiris-bridge/server.gd").new()
	_server.start(BIND_ADDR, BIND_PORT, get_editor_interface(), get_undo_redo())
	_controller = preload("res://addons/osiris-bridge/debugger_controller.gd").new()
	_server.set_controller(_controller)
	_controller.set_plugin(self)
	add_debugger_plugin(_controller)
	# Registra el autoload de input (una vez). El juego lo carga al arrancar y
	# el bridge escribe user://osiris_input.json sin depender del debugger.
	_ensure_input_autoload()
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
	_refresh_runtime()

var _last_refresh: int = 0

## Pide árbol y métricas periódicamente mientras el juego corre Y hay un cliente
## MCP conectado (el primer request puede perderse antes de que la sesión esté
## activa). Sin clientes, refrescar el runtime es trabajo muerto: `send_message`
## al juego cuesta, y nadie consume el buffer.
func _refresh_runtime() -> void:
	if _controller == null or _server == null:
		return
	if not _server.is_running():
		_requested_sessions.clear()
		return
	if not _server.has_clients():
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
##
## Las sesiones llegan por el hook `_setup_session` del controller (editor_
## debugger_plugin.cpp:157), que se llama justo al crear la sesión — sin
## polling. Este método solo conecta la señal a ese node.
func _on_session_created(session_node: Node) -> void:
	if not is_instance_valid(session_node):
		return
	if _connected_sessions.has(session_node.get_instance_id()):
		return
	_connected_sessions[session_node.get_instance_id()] = true
	session_node.connect("debug_data", Callable(_server, "_on_debug_data"))
	if _controller and not _requested_sessions.has(session_node.get_instance_id()):
		_requested_sessions[session_node.get_instance_id()] = true
		# Pide el árbol remoto y activa métricas una vez por sesión.
		_controller.request_tree()
		_controller.enable_metrics()

## Registra el autoload de input en project.godot (idempotente). El juego lo
## carga al arrancar y aplica el input escrito al archivo user://osiris_input.json.
func _ensure_input_autoload() -> void:
	var key := "autoload/OsirisInput"
	if ProjectSettings.has_setting(key):
		return
	var script_path := "res://addons/osiris-bridge/osiris_input_autoload.gd"
	if not FileAccess.file_exists(script_path):
		print("[OsirisBridge] no se encuentra %s; input.send no funcionará" % script_path)
		return
	ProjectSettings.set_setting(key, "*%s" % script_path)
	ProjectSettings.save()
