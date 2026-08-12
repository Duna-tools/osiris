extends Node

## Autoload de Osiris para inyección de input. Procesa user://osiris_input.json
## en _physics_process. Cada entry tiene un seq monótono que evita re-procesar;
## el archivo se borra tras el apply. Sin mtime, sin colas, sin race conditions.
##
## Formato del JSON (un entry por archivo, no array):
##   {"seq": 1, "action": "jump", "pressed": true}
##   {"seq": 2, "key": "W", "pressed": false}

var _last_seq: int = 0

func _physics_process(_delta: float) -> void:
	const path := "user://osiris_input.json"
	if not FileAccess.file_exists(path):
		return
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		return
	var raw: String = f.get_as_text()
	f.close()
	if raw.strip_edges() == "":
		return
	var json := JSON.new()
	if json.parse(raw) != OK:
		return
	var entry: Variant = json.get_data()
	if typeof(entry) != TYPE_DICTIONARY:
		return
	var seq: int = int(entry.get("seq", 0))
	if seq == 0 or seq <= _last_seq:
		return
	_last_seq = seq
	# Borrar ANTES de aplicar: si el apply falla, no re-procesamos el mismo
	# input (cada LLM call es un nuevo entry con seq distinto).
	DirAccess.remove_absolute(ProjectSettings.globalize_path(path))
	_apply(entry)

func _apply(entry: Dictionary) -> void:
	if entry.has("action"):
		var action: String = str(entry.get("action", ""))
		if bool(entry.get("pressed", false)):
			Input.action_press(action)
		else:
			Input.action_release(action)
		return
	var key_name: String = str(entry.get("key", ""))
	if key_name == "":
		return
	var keycode: Key = OS.find_keycode_from_string(key_name)
	if keycode == KEY_NONE:
		return
	var ev := InputEventKey.new()
	ev.keycode = keycode
	ev.pressed = bool(entry.get("pressed", false))
	Input.parse_input_event(ev)
