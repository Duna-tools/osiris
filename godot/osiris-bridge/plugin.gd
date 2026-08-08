@tool
extends EditorPlugin

const BIND_ADDR := "127.0.0.1"
const BIND_PORT := 9876

var _server: RefCounted = null

func _enter_tree() -> void:
	@warning_ignore("inferred_declaration")
	randomize()
	_server = preload("res://addons/osiris-bridge/server.gd").new()
	_server.start(BIND_ADDR, BIND_PORT, get_editor_interface())
	if FileAccess.file_exists("res://scenes/main.tscn"):
		get_editor_interface().open_scene_from_path("res://scenes/main.tscn")

func _exit_tree() -> void:
	if _server:
		_server.stop()
		_server = null

func _process(_delta: float) -> void:
	if _server:
		_server.poll()
