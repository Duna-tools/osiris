@tool
extends EditorPlugin

const BIND_ADDR := "127.0.0.1"
const BIND_PORT := 9876

var _server: RefCounted = null
var _debugger_node: Node = null
var _connected_sessions: Dictionary = {}

func _enter_tree() -> void:
	@warning_ignore("inferred_declaration")
	randomize()
	_server = preload("res://addons/osiris-bridge/server.gd").new()
	_server.start(BIND_ADDR, BIND_PORT, get_editor_interface(), get_undo_redo())
	if FileAccess.file_exists("res://scenes/main.tscn"):
		get_editor_interface().open_scene_from_path("res://scenes/main.tscn")

func _exit_tree() -> void:
	if _server:
		_server.stop()
		_server = null
	_connected_sessions.clear()

func _process(_delta: float) -> void:
	if _server:
		_server.poll()
	_attach_debugger()

## Encuentra el dock "Debugger" del editor y conecta la señal debug_data de cada
## sesión (ScriptEditorDebugger). La señal se emite para TODOS los mensajes del
## juego (vault 18 §4); error/output tienen handlers nativos, así que el plugin
## EditorDebuggerPlugin no los recibe, pero debug_data sí los ve siempre.
func _attach_debugger() -> void:
	if _debugger_node == null or not is_instance_valid(_debugger_node):
		_debugger_node = _find_debugger_node()
		_connected_sessions.clear()
	if _debugger_node == null:
		print("[OsirisBridge] debugger node no encontrado")
		return
	var attached: int = 0
	# Las sesiones viven dentro de un TabContainer hijo del dock.
	var sessions: Array = _debugger_node.find_children("*", "ScriptEditorDebugger", true, false)
	for child: Node in sessions:
		if _connected_sessions.has(child.get_instance_id()):
			continue
		_connected_sessions[child.get_instance_id()] = true
		child.connect("debug_data", Callable(_server, "_on_debug_data"))
		attached += 1
	if attached > 0:
		print("[OsirisBridge] debug_data conectado a %d sesión(es)" % attached)

func _find_debugger_node() -> Node:
	var base: Control = get_editor_interface().get_base_control()
	if base == null:
		print("[OsirisBridge] base control null")
		return null
	var by_class: Array = base.find_children("*", "EditorDebuggerNode", true, false)
	if by_class.size() > 0:
		print("[OsirisBridge] debugger por clase: %s" % by_class[0].name)
		return by_class[0]
	var by_name: Array = base.find_children("Debugger", "*", true, false)
	print("[OsirisBridge] por clase: %d, por nombre: %d" % [by_class.size(), by_name.size()])
	for n in by_name:
		print("[OsirisBridge]   candidato: %s (%s)" % [n.name, n.get_class()])
	return by_name[0] if by_name.size() > 0 else null
