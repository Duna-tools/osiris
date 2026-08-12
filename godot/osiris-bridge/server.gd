@tool
class_name OsirisBridgeServer
extends RefCounted

const PROTOCOL_VERSION := 1
const MAX_LINE_BYTES := 1048576
const TOKEN_FILE := "user://osiris_ipc_token"
const MAX_POLL_TIME_MSEC := 5
const MAX_MESSAGES_PER_POLL := 50

# Nombres de los 40 monitores estándar de Godot (main/performance.cpp:165).
const STANDARD_MONITORS := [
	"time/fps", "time/process", "time/physics_process", "time/navigation_process",
	"memory/static", "memory/static_max", "memory/msg_buf_max",
	"object/objects", "object/resources", "object/nodes", "object/orphan_nodes",
	"raster/total_objects_drawn", "raster/total_primitives_drawn", "raster/total_draw_calls",
	"video/video_mem", "video/texture_mem", "video/buffer_mem",
	"physics_2d/active_objects", "physics_2d/collision_pairs", "physics_2d/islands",
	"physics_3d/active_objects", "physics_3d/collision_pairs", "physics_3d/islands",
	"audio/driver/output_latency",
	"navigation/active_maps", "navigation/regions", "navigation/agents", "navigation/links",
	"navigation/polygons", "navigation/edges", "navigation/edges_merged",
	"navigation/edges_connected", "navigation/edges_free", "navigation/obstacles",
	"pipeline/compilations_canvas", "pipeline/compilations_mesh",
	"pipeline/compilations_surface", "pipeline/compilations_draw",
	"pipeline/compilations_specialization",
	"navigation_2d/active_maps", "navigation_2d/regions", "navigation_2d/agents",
	"navigation_2d/links", "navigation_2d/polygons", "navigation_2d/edges",
]

var _tcp: TCPServer = null
var _peers: Array = []
var _peer_buffers: Dictionary = {}
var _busy: bool = false
var _token: String = ""
var _editor: EditorInterface = null
var _undo_redo: EditorUndoRedoManager = null
var _run_deadline_ms: int = 0
var _debug_errors: Array = []
var _debug_output: Array = []
var _debug_tree: Array = []
var _debug_metric_names: Array = []
var _debug_metrics: Array = []
var _debug_inspect: Array = []
var _controller: Object = null
var _debug_screenshot_path: String = ""
const RUN_HISTORY_FILE := "user://osiris_run_history.jsonl"
const RUN_HISTORY_MAX := 200
var _run_start_ms: int = 0
var _run_started_scene: String = ""
# Registro de operaciones (ADR-008, espejo del core Rust): op -> handler.
# Evita el `match` gigante; añadir una op es una línea en _register_handlers.
var _handlers: Dictionary = {}
# Booster de rendimiento: mientras haya clientes conectados, el editor no debe
# throttlear el main loop (vsync/low_processor) o cada llamada esperaría un
# frame (6.9ms a 144Hz, ~100ms con ventana oculta en Wayland).
var _boosted: bool = false
var _saved_max_fps: int = 0
var _saved_low_proc: bool = false
var _saved_vsync: int = 0

func set_controller(c: Object) -> void:
	_controller = c

func is_running() -> bool:
	return _editor.is_playing_scene()

## ¿Hay algún cliente MCP conectado? El refresh de runtime solo debe correr con
## consumidor: sin clientes, pedir el árbol del juego cada 2s es trabajo muerto.
func has_clients() -> bool:
	return _peers.size() > 0

## Callback conectado por plugin.gd a la señal debug_data del ScriptEditorDebugger.
## Recibe TODOS los mensajes del juego ya decodificados (vault 18 §4).
func _on_debug_data(msg: String, data: Variant) -> void:
	if msg == "error":
		_append_error(data)
	elif msg == "output":
		_append_output(data)
	elif msg == "scene:scene_tree":
		_append_tree(data)
	elif msg == "performance:profile_names":
		_append_metric_names(data)
	elif msg == "performance:profile_frame":
		_debug_metrics = data
	elif msg == "scene:inspect_objects":
		_debug_inspect = data
	elif msg == "game_view:get_screenshot":
		# [id, w, h, path] — el juego ya guardó el PNG en temp (scene_debugger.cpp:534).
		if typeof(data) == TYPE_ARRAY and data.size() >= 4:
			_debug_screenshot_path = data[3]

func _append_tree(data: Variant) -> void:
	if typeof(data) != TYPE_ARRAY:
		return
	# Flat: [child_count, name, type, id, scene_file_path, view_flags] por nodo (DFS).
	var nodes: Array = []
	var i: int = 0
	while i + 5 < data.size():
		nodes.append({
			"child_count": data[i],
			"name": data[i + 1],
			"type": data[i + 2],
			"id": data[i + 3],
			"scene_file": data[i + 4],
		})
		i += 6
	_debug_tree = nodes

func _append_metric_names(data: Variant) -> void:
	if typeof(data) != TYPE_ARRAY or data.size() < 1:
		return
	_debug_metric_names = data[0]

func _append_error(data: Variant) -> void:
	if typeof(data) != TYPE_ARRAY or data.size() < 11:
		return
	# [hr, min, sec, msec, file, func, line, error, error_descr, warning, stack_size, ...]
	var entry := {
		"file": data[4],
		"func": data[5],
		"line": data[6],
		"message": data[7],
		"descr": data[8],
		"warning": data[9],
	}
	_debug_errors.append(entry)
	if _debug_errors.size() > 200:
		_debug_errors.pop_front()

func _append_output(data: Variant) -> void:
	if typeof(data) != TYPE_ARRAY or data.size() < 1:
		return
	# [PackedStringArray líneas, PackedInt32Array tipos]
	var lines: Array = data[0] if typeof(data[0]) == TYPE_PACKED_STRING_ARRAY or typeof(data[0]) == TYPE_ARRAY else []
	for line in lines:
		_debug_output.append(String(line))
		if _debug_output.size() > 500:
			_debug_output.pop_front()

func start(addr: String, port: int, editor: EditorInterface, undo_redo: EditorUndoRedoManager) -> void:
	_tcp = TCPServer.new()
	if _tcp.listen(port, addr) != OK:
		push_error("[OsirisBridge] no se puede escuchar en %s:%d" % [addr, port])
		_tcp = null
		return
	_token = _generate_token()
	_write_token()
	_editor = editor
	_undo_redo = undo_redo
	_register_handlers()
	print_rich("[OsirisBridge] escuchando en %s:%d" % [addr, port])

## Registro de operaciones (Registry/Command, ADR-008). Una línea por op.
func _register_handlers() -> void:
	_handlers = {
		"ping": _on_ping,
		"get_version": _get_version,
		"get_state": _get_state,
		"get_active_scene": _get_active_scene,
		"list_nodes": _list_nodes,
		"get_property": _get_property,
		"list_resources": _list_resources,
		"get_editor_errors": _get_editor_errors,
		"create_node": _create_node,
		"create_tween": _create_tween,
		"set_description": _set_description,
		"set_unique_name": _set_unique_name,
		"reparent_node": _reparent_node,
		"duplicate_node": _duplicate_node,
		"instance_scene": _instance_scene,
		"inputmap_add": _inputmap_add,
		"scene_apply": _scene_apply,
		"remove_node": _remove_node,
		"set_property": _set_property,
		"object_property_get": _object_property_get,
		"object_property_set": _object_property_set,
		"call_method": _call_method,
		"save_scene": _save_scene,
		"save_scene_as": _save_scene_as,
		"save_all_scenes": _save_all_scenes,
		"unsaved_scenes": _unsaved_scenes,
		"inspect_node": _inspect_node,
		"open_scene": _open_scene,
		"open_scenes": _open_scenes,
		"move_node": _move_node,
		"create_scene": _create_scene,
		"export_project": _export_project,
		"run_project": _run_project,
		"input_send": _input_send,
		"stop_project": _stop_project,
		"reload_editor": _reload_editor,
		"get_run_status": _get_run_status,
		"get_run_output": _get_run_output,
		"run_history": _run_history,
		"assert_property": _assert_property,
		"validate_script": _validate_script,
		"write_script": _write_script,
		"describe_class": _describe_class,
		"list_signals": _list_signals,
		"connect_signal": _connect_signal,
		"disconnect_signal": _disconnect_signal,
		"emit_signal": _emit_signal,
		"add_to_group": _add_to_group,
		"remove_from_group": _remove_from_group,
		"get_groups": _get_groups,
		"nodes_in_group": _nodes_in_group,
		"input_query": _input_query,
		"asset_list": _asset_list,
		"project_files": _project_files,
		"project_settings_get": _project_settings_get,
		"project_settings_set": _project_settings_set,
		"editor_settings_get": _editor_settings_get,
		"editor_settings_set": _editor_settings_set,
		"asset_reimport": _asset_reimport,
		"resource_assign": _resource_assign,
		"resource_create": _resource_create,
		"runtime_tree": _runtime_tree,
		"runtime_metrics": _runtime_metrics,
		"debug_inspect": _handle_debug_inspect,
		"runtime_capture": _runtime_capture,
		"select_nodes": _select_nodes,
		"editor_screenshot": _editor_screenshot,
		"add_autoload": _add_autoload,
	}

func _on_ping(id: Variant, params: Dictionary = {}) -> Dictionary:
	return _ok(id, {"pong": true})


func stop() -> void:
	for p: StreamPeerTCP in _peers:
		p.disconnect_from_host()
	_peers.clear()
	_peer_buffers.clear()
	if _boosted:
		_boosted = false
		if DisplayServer.get_name() != "headless":
			Engine.max_fps = _saved_max_fps
			OS.low_processor_usage_mode = _saved_low_proc
			DisplayServer.window_set_vsync_mode(_saved_vsync)
	if _tcp:
		_tcp.stop()
		_tcp = null

func _update_boost() -> void:
	# Sobre el main loop del editor (solo si hay una ventana real).
	# 0 fps = sin techo; sin vsync el loop corre tan rápido como pueda.
	var active: bool = _peers.size() > 0
	if active == _boosted:
		return
	if active:
		_saved_max_fps = Engine.get_max_fps()
		_saved_low_proc = OS.low_processor_usage_mode
		_saved_vsync = DisplayServer.window_get_vsync_mode()
		if DisplayServer.get_name() != "headless":
			Engine.max_fps = 0
			OS.low_processor_usage_mode = false
			DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	else:
		if DisplayServer.get_name() != "headless":
			Engine.max_fps = _saved_max_fps
			OS.low_processor_usage_mode = _saved_low_proc
			DisplayServer.window_set_vsync_mode(_saved_vsync)
	_boosted = active

func poll() -> void:
	if _busy:
		return
	_busy = true
	# Timeout de ejecución real (segundos), no frames (RF-09).
	if _run_deadline_ms > 0 and Time.get_ticks_msec() >= _run_deadline_ms:
		print("[OsirisBridge] timeout poll: %d vs deadline %d" % [Time.get_ticks_msec(), _run_deadline_ms])
		_run_deadline_ms = 0
		_editor.stop_playing_scene()
		_record_run_stop("timeout")
	elif _run_start_ms > 0 and not _editor.is_playing_scene():
		# El juego terminó solo; registrar el fin una vez.
		_record_run_stop("finished")
	_accept()
	var start_time: int = Time.get_ticks_msec()
	var processed_count: int = 0
	var to_remove: Array = []
	for p: StreamPeerTCP in _peers:
		if p.get_status() != StreamPeerTCP.STATUS_CONNECTED:
			to_remove.append(p)
		else:
			processed_count += _process_peer(p)
			if Time.get_ticks_msec() - start_time >= MAX_POLL_TIME_MSEC or processed_count >= MAX_MESSAGES_PER_POLL:
				break
	for p in to_remove:
		_peers.erase(p)
		_peer_buffers.erase(p)
	_update_boost()
	_busy = false


func _accept() -> void:
	while _tcp and _tcp.is_connection_available():
		var p: StreamPeerTCP = _tcp.take_connection()
		if p:
			_peers.append(p)
	_update_boost()

func _process_peer(peer: StreamPeerTCP) -> int:
	# Drena el buffer completo: procesa todos los mensajes \n disponibles en el
	# mismo poll, no uno por frame. Evita +1 frame de latencia por mensaje en
	# ráfagas (varios tools/call encolados).
	var buf: String = _peer_buffers.get(peer, "")
	var avail: int = peer.get_available_bytes()
	if avail > 0:
		buf += peer.get_utf8_string(avail)
	var processed: int = 0
	var start_time: int = Time.get_ticks_msec()
	while true:
		if buf.length() > MAX_LINE_BYTES:
			_peer_buffers.erase(peer)
			break
		var newline: int = buf.find("\n")
		if newline < 0:
			_peer_buffers[peer] = buf
			break
		var line: String = buf.substr(0, newline).strip_edges()
		buf = buf.substr(newline + 1)
		var msg: Variant = _parse_line(line)
		if typeof(msg) == TYPE_DICTIONARY:
			var response: Variant = _dispatch(msg)
			if response:
				_send(peer, response)
				processed += 1
		# Cap de tiempo: no acaparar el frame del editor con una ráfaga enorme.
		if Time.get_ticks_msec() - start_time >= MAX_POLL_TIME_MSEC:
			_peer_buffers[peer] = buf
			break
	return processed

func _parse_line(line: String) -> Variant:
	if line == "":
		return null
	var json := JSON.new()
	if json.parse(line) != OK:
		return null
	var msg: Variant = json.get_data()
	if typeof(msg) != TYPE_DICTIONARY:
		return null
	return msg

func _send(peer: StreamPeerTCP, data: Dictionary) -> void:
	var txt: String = JSON.stringify(data, "", false)
	peer.put_data((txt + "\n").to_utf8_buffer())

func _dispatch(msg: Dictionary) -> Variant:
	var typ: String = msg.get("type", "")
	match typ:
		"hello":
			return _hello(msg)
		"request":
			return _request(msg)
		"cancel":
			return null
		_:
			return _error(null, "validation", "tipo desconocido: %s" % typ)

func _hello(msg: Dictionary) -> Dictionary:
	var pversion: int = msg.get("protocol", 0)
	if pversion != PROTOCOL_VERSION:
		return _hello_err("compatibility", "protocolo %d, se espera %d" % [pversion, PROTOCOL_VERSION])
	var tok: String = msg.get("token", "")
	# Comparación simple de strings local: defensa de proceso, no criptográfica (FND-008).
	if tok != _token:
		return _hello_err("authentication", "token inválido")
	var vi: Dictionary = Engine.get_version_info()
	var ver: String = "%d.%d.%d" % [vi.major, vi.minor, vi.patch]
	return {"type": "hello_ack", "protocol": PROTOCOL_VERSION, "godot": ver}

func _hello_err(code: String, message: String) -> Dictionary:
	return {"type": "hello_error", "code": code, "message": message}

func _request(msg: Dictionary) -> Dictionary:
	var id: Variant = msg.get("id")
	var op: String = msg.get("op", "")
	var handler: Callable = _handlers.get(op, Callable())
	if not handler.is_valid():
		return _error(id, "validation", "operación desconocida: %s" % op, false)
	return handler.call(id, msg.get("params", {}) if typeof(msg.get("params")) == TYPE_DICTIONARY else {})

func _list_signals(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var signals: Array = []
	for s: Dictionary in node.get_signal_list():
		var args: Array = []
		for a: Dictionary in s.get("args", []):
			var type_name: String = a.get("class_name", "") if a.get("class_name", "") != "" else type_string(a.get("type", TYPE_NIL))
			args.append({"name": a.get("name", ""), "type": type_name})
		signals.append({"name": s.get("name", ""), "args": args})
	return _ok(id, {"path": params.get("path", "."), "signals": signals})

func _connect_signal(id: Variant, params: Dictionary) -> Dictionary:
	var source: Node = _resolve_node(params)
	if not source:
		return _error(id, "godot", "nodo fuente no encontrado")
	var signal_name: String = params.get("signal", "")
	if signal_name == "":
		return _error(id, "validation", "parámetro 'signal' requerido")
	var target: Node = _resolve_target_node(params)
	if not target:
		return _error(id, "godot", "nodo destino no encontrado")
	var method: String = params.get("method", "")
	if method == "":
		return _error(id, "validation", "parámetro 'method' requerido")
	if source.is_connected(signal_name, Callable(target, method)):
		return _ok(id, {"already_connected": true})
	var flags: int = 0
	if bool(params.get("deferred", false)):
		flags |= Object.CONNECT_DEFERRED
	if bool(params.get("oneshot", false)):
		flags |= Object.CONNECT_ONE_SHOT
	var err: int = source.connect(signal_name, Callable(target, method), flags)
	if err != OK:
		return _error(id, "godot", "no se pudo conectar %s -> %s (%d)" % [signal_name, method, err])
	return _ok(id, {"connected": true, "signal": signal_name, "target_path": params.get("target_path", "."), "method": method})

func _disconnect_signal(id: Variant, params: Dictionary) -> Dictionary:
	var source: Node = _resolve_node(params)
	if not source:
		return _error(id, "godot", "nodo fuente no encontrado")
	var signal_name: String = params.get("signal", "")
	if signal_name == "":
		return _error(id, "validation", "parámetro 'signal' requerido")
	var target: Node = _resolve_target_node(params)
	if not target:
		return _error(id, "godot", "nodo destino no encontrado")
	var method: String = params.get("method", "")
	if method == "":
		return _error(id, "validation", "parámetro 'method' requerido")
	if not source.is_connected(signal_name, Callable(target, method)):
		return _ok(id, {"disconnected": false, "note": "no estaba conectada"})
	source.disconnect(signal_name, Callable(target, method))
	return _ok(id, {"disconnected": true, "signal": signal_name, "target_path": params.get("target_path", "."), "method": method})

## Emite una señal en un nodo (Object.emit_signal, bound nativo).
func _emit_signal(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var signal_name: String = params.get("signal", "")
	if signal_name == "":
		return _error(id, "validation", "parámetro 'signal' requerido")
	var raw_args: Array = params.get("args", [])
	if raw_args.size() > 0:
		var args: Array = []
		for a in raw_args:
			args.append(_json_to_variant(a))
		# emit_signal no acepta arrays; pasar hasta 4 argumentos posicionales.
		match args.size():
			1: node.emit_signal(signal_name, args[0])
			2: node.emit_signal(signal_name, args[0], args[1])
			3: node.emit_signal(signal_name, args[0], args[1], args[2])
			_: node.emit_signal(signal_name, args[0], args[1], args[2], args[3])
	else:
		node.emit_signal(signal_name)
	return _ok(id, {"emitted": signal_name, "path": params.get("path", ".")})

## Añade un nodo a un grupo (Node.add_to_group, bound).
func _add_to_group(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var group: String = params.get("group", "")
	if group == "":
		return _error(id, "validation", "parámetro 'group' requerido")
	node.add_to_group(group)
	return _ok(id, {"group": group, "path": params.get("path", ".")})

## Quita un nodo de un grupo (Node.remove_from_group, bound).
func _remove_from_group(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var group: String = params.get("group", "")
	if group == "":
		return _error(id, "validation", "parámetro 'group' requerido")
	node.remove_from_group(group)
	return _ok(id, {"group": group, "path": params.get("path", ".")})

## Lista los grupos de un nodo (Node.get_groups, bound).
func _get_groups(id: Variant, params: Dictionary = {}) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var groups: Array = []
	for g in node.get_groups():
		groups.append(String(g))
	return _ok(id, {"path": params.get("path", "."), "groups": groups})

## Query de nodos por grupo (SceneTree.get_nodes_in_group, bound).
func _nodes_in_group(id: Variant, params: Dictionary) -> Dictionary:
	var group: String = params.get("group", "")
	if group == "":
		return _error(id, "validation", "parámetro 'group' requerido")
	var nodes: Array = []
	for n in _editor.get_edited_scene_root().get_tree().get_nodes_in_group(group):
		nodes.append({"name": n.name, "class": n.get_class()})
	return _ok(id, {"group": group, "nodes": nodes, "count": nodes.size()})

## Consulta el estado del input en el editor (útil para debug). Devuelve
## is_action_pressed, get_vector, is_key_pressed para las teclas/acciones pedidas.
func _input_query(id: Variant, params: Dictionary) -> Dictionary:
	var actions: Array = params.get("actions", [])
	var keys: Array = params.get("keys", [])
	var result: Dictionary = {}
	for a in actions:
		result[str(a)] = Input.is_action_pressed(str(a))
	var vec_name: String = params.get("vector", "")
	if vec_name != "":
		var dir := Input.get_vector(
			"%s_left" % vec_name, "%s_right" % vec_name,
			"%s_up" % vec_name, "%s_down" % vec_name
		)
		result["vector_%s" % vec_name] = {"x": dir.x, "y": dir.y}
	for k in keys:
		var keycode: Key = OS.find_keycode_from_string(str(k))
		result[str(k)] = Input.is_key_pressed(keycode) if keycode != KEY_NONE else false
	return _ok(id, {"input": result})

func _resolve_target_node(params: Dictionary) -> Node:
	var root: Node = _editor.get_edited_scene_root()
	var raw: String = params.get("target_path", "")
	if raw == "":
		return null
	var node_path: String = _validate_path(raw)
	if node_path == "":
		return null
	return root.get_node_or_null(NodePath(node_path))

func _project_files(id: Variant, params: Dictionary) -> Dictionary:
	# Lista archivos fuente del proyecto vía DirAccess (sin depender del scan del
	# EditorFileSystem). Excluye .godot/ y addons/ (infra del editor/puente).
	var max_total: int = clampi(params.get("max_total", 500), 1, 2000)
	var items: Array = []
	_collect_project_files("res://", items, max_total)
	return _ok(id, {"path": "res://", "items": items, "has_more": items.size() >= max_total})

func _collect_project_files(dir_path: String, items: Array, max_total: int) -> void:
	var dir: DirAccess = DirAccess.open(dir_path)
	if dir == null:
		return
	dir.list_dir_begin()
	var entry: String = dir.get_next()
	while entry != "":
		if items.size() >= max_total:
			break
		if entry.begins_with(".") or entry == "addons":
			entry = dir.get_next()
			continue
		var full: String = dir_path + entry
		if dir.current_is_dir():
			_collect_project_files(full + "/", items, max_total)
		else:
			var f: FileAccess = FileAccess.open(full, FileAccess.READ)
			items.append({
				"path": full,
				"extension": entry.get_extension(),
				"size": f.get_length() if f else 0,
			})
			if f:
				f.close()
		entry = dir.get_next()
	dir.list_dir_end()

func _project_settings_get(id: Variant, params: Dictionary) -> Dictionary:
	var names: Array = params.get("names", [])
	var single: String = params.get("name", "")
	if single != "":
		names.append(single)
	if names.size() == 0:
		return _error(id, "validation", "parámetro 'names' o 'name' requerido")
	var settings: Dictionary = {}
	for n in names:
		var key: String = str(n)
		settings[key] = _variant_to_json(ProjectSettings.get_setting_with_override(key))
	return _ok(id, {"settings": settings})

func _project_settings_set(id: Variant, params: Dictionary) -> Dictionary:
	var name: String = params.get("name", "")
	if name == "":
		return _error(id, "validation", "parámetro 'name' requerido")
	var value: Variant = params.get("value")
	ProjectSettings.set_setting(name, _json_to_variant(value))
	var err: int = ProjectSettings.save()
	if err != OK:
		return _error(id, "godot", "no se pudo guardar project.godot (código %d)" % err)
	return _ok(id, {"name": name, "saved": true, "value": _variant_to_json(ProjectSettings.get_setting_with_override(name))})

## Lee una setting del editor (EditorSettings.get_setting, bound).
func _editor_settings_get(id: Variant, params: Dictionary) -> Dictionary:
	var name: String = params.get("name", "")
	if name == "":
		return _error(id, "validation", "parámetro 'name' requerido")
	var settings: EditorSettings = _editor.get_editor_settings()
	if not settings.has_setting(name):
		return _error(id, "validation", "setting desconocida: %s" % name)
	return _ok(id, {"name": name, "value": _variant_to_json(settings.get_setting(name))})

## Escribe una setting del editor (EditorSettings.set_setting, bound).
func _editor_settings_set(id: Variant, params: Dictionary) -> Dictionary:
	var name: String = params.get("name", "")
	if name == "":
		return _error(id, "validation", "parámetro 'name' requerido")
	var settings: EditorSettings = _editor.get_editor_settings()
	settings.set_setting(name, _json_to_variant(params.get("value")))
	return _ok(id, {"name": name, "value": _variant_to_json(settings.get_setting(name))})

func _asset_list(id: Variant, params: Dictionary) -> Dictionary:
	var fs: EditorFileSystem = _editor.get_resource_filesystem()
	var dir: EditorFileSystemDirectory = fs.get_filesystem()
	var rel: String = params.get("path", "res://")
	var max_total: int = clampi(params.get("max_total", 500), 1, 2000)
	var items: Array = []
	var has_more: bool = _collect_assets(dir, rel, items, max_total)
	return _ok(id, {"path": rel, "items": items, "has_more": has_more, "scanning": fs.is_scanning()})

func _collect_assets(dir: EditorFileSystemDirectory, rel: String, items: Array, max_total: int) -> bool:
	var dir_path: String = dir.get_path()
	if rel == "res://" or dir_path.begins_with(rel):
		for i in dir.get_file_count():
			if items.size() >= max_total:
				return true
			var path: String = dir.get_file_path(i)
			items.append({
				"path": path,
				"type": dir.get_file_type(i),
				"imported": dir.get_file_import_is_valid(i),
			})
	for i in dir.get_subdir_count():
		if items.size() >= max_total:
			return true
		var sub: EditorFileSystemDirectory = dir.get_subdir(i)
		var sub_path: String = sub.get_path()
		if rel == "res://" or sub_path.begins_with(rel):
			if _collect_assets(sub, rel, items, max_total):
				return true
	return false

func _asset_reimport(id: Variant, params: Dictionary) -> Dictionary:
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido")
	if not path.begins_with("res://"):
		return _error(id, "validation", "ruta debe comenzar con res://")
	var fs: EditorFileSystem = _editor.get_resource_filesystem()
	fs.update_file(path)
	return _ok(id, {"path": path, "queued": true})

func _resource_assign(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "parámetro 'property' requerido")
	var res_path: String = params.get("resource", "")
	if res_path == "":
		return _error(id, "validation", "parámetro 'resource' requerido")
	if not res_path.begins_with("res://"):
		return _error(id, "validation", "recurso debe comenzar con res://")
	var res: Resource = load(res_path)
	if not res:
		return _error(id, "godot", "no se pudo cargar el recurso: %s" % res_path)
	var old_value: Variant = node.get(prop)
	node.set(prop, res)
	_undo_redo.create_action("Osiris: asignar %s" % prop)
	_undo_redo.add_do_method(node, "set", prop, res)
	_undo_redo.add_undo_method(node, "set", prop, old_value)
	_undo_redo.commit_action()
	return _ok(id, {"property": prop, "resource": res_path, "assigned": true})

## Crea un recurso nuevo (Animation, Theme, SpriteFrames, ShaderMaterial...) y lo
## guarda a res://. ClassDB.instantiate (class_db.cpp:577) + ResourceSaver.save.
func _resource_create(id: Variant, params: Dictionary) -> Dictionary:
	var res_type: String = params.get("type", "")
	if res_type == "":
		return _error(id, "validation", "parámetro 'type' requerido (ej: Animation, Theme, SpriteFrames)")
	if not ClassDB.class_exists(res_type):
		return _error(id, "validation", "clase desconocida: %s" % res_type)
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido (res://*.tres)")
	if not path.begins_with("res://"):
		return _error(id, "validation", "ruta debe comenzar con res://")
	if FileAccess.file_exists(path):
		return _error(id, "conflict", "el recurso ya existe: %s" % path)
	# Crear el directorio padre si no existe (ResourceSaver.save falla si no está).
	var abs: String = ProjectSettings.globalize_path(path)
	var base_dir: String = abs.get_base_dir()
	if not DirAccess.dir_exists_absolute(base_dir):
		DirAccess.make_dir_recursive_absolute(base_dir)
	var res: Resource = ClassDB.instantiate(res_type)
	if not res or not res is Resource:
		return _error(id, "godot", "el tipo %s no es un Resource" % res_type)
	# Propiedades iniciales (opcional): dict nombre -> valor.
	var props: Variant = params.get("properties", {})
	if typeof(props) == TYPE_DICTIONARY:
		for key: Variant in props:
			res.set(str(key), _json_to_variant(props[key]))
	res.resource_path = path
	var err: int = ResourceSaver.save(res, path, ResourceSaver.FLAG_REPLACE_SUBRESOURCE_PATHS)
	if err != OK:
		return _error(id, "godot", "ResourceSaver.save devolvió %d" % err)
	return _ok(id, {"created": path, "type": res_type})

func _describe_class(id: Variant, params: Dictionary) -> Dictionary:
	var klass: String = params.get("class", "")
	if klass == "":
		return _error(id, "validation", "parámetro 'class' requerido")
	if not ClassDB.class_exists(klass):
		return _error(id, "validation", "clase desconocida: %s" % klass)
	var recursive: bool = params.get("recursive", false)
	var max_props: int = clampi(params.get("max_props", 300), 1, 2000)
	var max_methods: int = clampi(params.get("max_methods", 300), 1, 2000)

	var properties: Array = []
	for p: Dictionary in ClassDB.class_get_property_list(klass, not recursive):
		if properties.size() >= max_props:
			break
		var type_name: String = p.get("class_name", "") if p.get("class_name", "") != "" else type_string(p.get("type", TYPE_NIL))
		properties.append({"name": p.get("name", ""), "type": type_name})

	var methods: Array = []
	for m: Dictionary in ClassDB.class_get_method_list(klass, not recursive):
		if methods.size() >= max_methods:
			break
		var args: Array = []
		for a: Dictionary in m.get("args", []):
			var type_name: String = a.get("class_name", "") if a.get("class_name", "") != "" else type_string(a.get("type", TYPE_NIL))
			args.append({"name": a.get("name", ""), "type": type_name})
		methods.append({"name": m.get("name", ""), "args": args})

	var signals: Array = []
	for s: Dictionary in ClassDB.class_get_signal_list(klass, not recursive):
		signals.append(s.get("name", ""))

	var constants: Array = []
	for c: String in ClassDB.class_get_integer_constant_list(klass, not recursive):
		constants.append({"name": c, "value": ClassDB.class_get_integer_constant(klass, c)})

	return _ok(id, {
		"class": klass,
		"inherits": ClassDB.get_parent_class(klass),
		"properties": properties,
		"methods": methods,
		"signals": signals,
		"constants": constants,
	})

func _write_script(id: Variant, params: Dictionary) -> Dictionary:
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido")
	if not path.begins_with("res://") or path.get_extension() != "gd":
		return _error(id, "validation", "ruta debe ser res://*.gd")
	var code: String = params.get("code", "")
	if code == "":
		return _error(id, "validation", "parámetro 'code' requerido")
	var script := GDScript.new()
	script.source_code = code
	if script.reload() != OK:
		return _error(id, "validation", "error de sintaxis en el script; valida antes de escribir")
	var f: FileAccess = FileAccess.open(path, FileAccess.WRITE)
	if not f:
		return _error(id, "godot", "no se pudo abrir %s para escritura" % path)
	f.store_string(code)
	f.close()
	_editor.get_resource_filesystem().update_file(path)
	return _ok(id, {"path": path, "written": true, "chars": code.length()})

func _validate_script(id: Variant, params: Dictionary) -> Dictionary:
	var code: String = params.get("code", "")
	if code == "":
		return _error(id, "validation", "parámetro 'code' requerido")
	# Primero un reload rápido (parse interno) para el caso sin errores.
	var script := GDScript.new()
	script.source_code = code
	if script.reload() == OK:
		return _ok(id, {"valid": true})
	# Con error, lanza `godot --check-only` sobre el código en disco para obtener
	# el mensaje con la línea exacta (el reload interno no expone el detalle).
	var tmp_path: String = "res://.osiris_validate_tmp.gd"
	var w: FileAccess = FileAccess.open(tmp_path, FileAccess.WRITE)
	if not w:
		return _error(id, "godot", "no se pudo crear el temporal de validación")
	w.store_string(code)
	w.close()
	var out: Array = []
	var exit_code: int = OS.execute(
		OS.get_executable_path(),
		["--headless", "--path", ProjectSettings.globalize_path("res://"), "--check-only", "--script", tmp_path],
		out,
		true,
	)
	var errors: Array = _parse_check_errors(out)
	var tmp_abs: String = ProjectSettings.globalize_path(tmp_path)
	if DirAccess.dir_exists_absolute(tmp_abs.get_base_dir()):
		DirAccess.remove_absolute(tmp_abs)
	if exit_code == 0:
		return _ok(id, {"valid": true})
	# El detalle (mensaje + línea) se devuelve como resultado, no como error de
	# dominio: FND-004 limita el mensaje MCP al código estable, y el LLM necesita
	# saber DÓNDE está el error para corregirlo.
	return _ok(id, {"valid": false, "errors": errors})

## Extrae los errores "SCRIPT ERROR: ... at: <archivo>:<línea>" del stderr.
func _parse_check_errors(out: Array) -> Array:
	var errors: Array = []
	var stderr: String = ""
	for o in out:
		if typeof(o) == TYPE_STRING:
			stderr += str(o)
	var pending: Dictionary = {}
	for line: String in stderr.split("\n"):
		var line_txt: String = line.strip_edges()
		if line_txt.begins_with("SCRIPT ERROR:"):
			var msg: String = line_txt.trim_prefix("SCRIPT ERROR:")
			pending = {"message": msg.strip_edges()}
		elif line_txt.begins_with("at:") and not pending.is_empty():
			var at: String = line_txt.trim_prefix("at:")
			# Solo conservar el error si su "at:" apunta al archivo validado
			# (evita el ruido de la carga interna de Godot con su propia línea).
			if at.contains(".osiris_validate_tmp.gd:"):
				var line_no: int = 0
				var m := RegEx.create_from_string(r":(\d+)")
				var m_res := m.search(at)
				if m_res:
					line_no = int(m_res.get_string(1))
				pending["line"] = line_no
				errors.append(pending)
			pending = {}
	return errors


func _get_version(id: Variant, params: Dictionary = {}) -> Dictionary:
	var vi: Dictionary = Engine.get_version_info()
	var info: Dictionary = {
		"godot": "%d.%d.%d" % [vi.major, vi.minor, vi.patch],
		"project": ProjectSettings.get_setting("application/config/name", ""),
	}
	return _ok(id, info)

## Estado completo de la máquina (DFA): el LLM lee aquí dónde está antes de
## elegir la transición. Consolida editor + juego + sesión debugger + input.
func _get_state(id: Variant, params: Dictionary = {}) -> Dictionary:
	var vi: Dictionary = Engine.get_version_info()
	var root: Node = _editor.get_edited_scene_root()
	var running: bool = _editor.is_playing_scene()
	# Sesión del debugger "connected" si el árbol remoto llegó (runtime.tree lo
	# llena); pending si el juego corre pero aún no; none si no corre.
	var debugger_session: String = "none"
	if running:
		debugger_session = "connected" if _debug_tree.size() > 0 else "pending"
	var input_node: String = "absent"
	if ProjectSettings.has_setting("autoload/%s" % OSIRIS_INPUT_AUTOLOAD_NAME):
		input_node = "autoload"
	var scenes_open: Array = []
	for s in _editor.get_open_scenes():
		scenes_open.append(String(s))
	return _ok(id, {
		"editor": {
			"connected": true,
			"godot_version": "%d.%d.%d" % [vi.major, vi.minor, vi.patch],
			"project": ProjectSettings.get_setting("application/config/name", ""),
			"scene_active": root.scene_file_path if root else "",
			"scenes_open": scenes_open,
			"unsaved": _editor.get_unsaved_scenes().size() > 0,
		},
		"game": {
			"running": running,
			"scene": _run_started_scene if running else "",
			"debugger_session": debugger_session,
			"input_node": input_node,
			"errors_count": _debug_errors.size(),
		},
	})

func _get_active_scene(id: Variant, params: Dictionary = {}) -> Dictionary:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _error(id, "godot", "no hay escena activa")
	return _ok(id, {
		"path": root.scene_file_path,
		"root_name": root.name,
		"node_count": _count_nodes(root),
	})

func _list_nodes(id: Variant, params: Dictionary) -> Dictionary:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _error(id, "godot", "no hay escena activa")
	var start_path: String = params.get("path", ".")
	var start: Node = root
	if start_path != ".":
		start = root.get_node_or_null(NodePath(start_path))
		if not start:
			return _error(id, "validation", "ruta no encontrada: %s" % start_path)
	var depth: int = clampi(params.get("depth", 3), 0, 20)
	var max_total: int = clampi(params.get("max_total", 500), 1, 2000)
	var items: Array = []
	var has_more: bool = _collect_nodes(start, depth, items, max_total)
	return _ok(id, {
		"items": items,
		"has_more": has_more,
	})

func _count_nodes(node: Node) -> int:
	var n: int = 1
	for c in node.get_children():
		n += _count_nodes(c)
	return n

func _collect_nodes(node: Node, max_depth: int, items: Array, max_total: int, depth: int = 0) -> bool:
	for child: Node in node.get_children():
		if items.size() >= max_total:
			return true
		items.append({
			"name": child.name,
			"type": child.get_class(),
			"child_count": child.get_child_count(),
			"depth": depth + 1,
		})
		if depth + 1 < max_depth:
			if _collect_nodes(child, max_depth, items, max_total, depth + 1):
				return true
	return false

func _ok(id: Variant, payload: Variant) -> Dictionary:
	# JSON.parse convierte todo número a float; el contrato espera id entero.
	var id_out: Variant = int(id) if id != null else null
	return {"type": "response", "id": id_out, "ok": payload}

func _error(id: Variant, code: String, message: String, retryable: bool = false) -> Dictionary:
	var id_out: Variant = int(id) if id != null else null
	return {"type": "response", "id": id_out, "error": {"code": code, "message": message, "retryable": retryable}}

func _get_property(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "parámetro 'property' requerido")
	var value: Variant = node.get(prop)
	return _ok(id, {
		"path": params.get("path", "."),
		"property": prop,
		"value": _variant_to_json(value),
	})

func _list_resources(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var resources: Array = []
	for prop: Dictionary in node.get_property_list():
		if prop.type == TYPE_OBJECT and (prop.usage & PROPERTY_USAGE_STORAGE):
			var val: Variant = node.get(prop.name)
			if val and val is Resource:
				resources.append({
					"property": prop.name,
					"path": val.resource_path,
					"class": val.get_class(),
				})
	return _ok(id, {"resources": resources})

func _variant_to_json(v: Variant, depth: int = 4) -> Variant:
	if depth <= 0:
		return "<max depth>"
	match typeof(v):
		TYPE_NIL: return null
		TYPE_BOOL: return v
		TYPE_INT, TYPE_FLOAT: return v
		TYPE_STRING, TYPE_STRING_NAME: return v
		TYPE_VECTOR2: return {"x": v.x, "y": v.y}
		TYPE_VECTOR3: return {"x": v.x, "y": v.y, "z": v.z}
		TYPE_VECTOR4: return {"x": v.x, "y": v.y, "z": v.z, "w": v.w}
		TYPE_COLOR: return "#%02x%02x%02x%02x" % [int(v.r*255), int(v.g*255), int(v.b*255), int(v.a*255)]
		TYPE_ARRAY, TYPE_PACKED_BYTE_ARRAY, TYPE_PACKED_INT32_ARRAY, TYPE_PACKED_FLOAT32_ARRAY, TYPE_PACKED_STRING_ARRAY, TYPE_PACKED_VECTOR2_ARRAY, TYPE_PACKED_VECTOR3_ARRAY, TYPE_PACKED_COLOR_ARRAY, TYPE_PACKED_VECTOR4_ARRAY:
			var arr: Array = []
			# Sin truncado: el límite real es MAX_LINE_BYTES del protocolo. Truncar
			# a 100 perdía datos silenciosamente (ej: arrays grandes de props).
			for i in v.size():
				arr.append(_variant_to_json(v[i], depth - 1))
			return arr
		TYPE_DICTIONARY:
			var d: Dictionary = {}
			for key: Variant in v:
				d[str(key)] = _variant_to_json(v[key], depth - 1)
			return d
		TYPE_OBJECT:
			return str(v)
		TYPE_RECT2: return {"position": _variant_to_json(v.position, 1), "size": _variant_to_json(v.size, 1)}
		TYPE_TRANSFORM2D: return {"x": _variant_to_json(v.x, 1), "y": _variant_to_json(v.y, 1), "origin": _variant_to_json(v.origin, 1)}
		TYPE_PLANE: return {"normal": _variant_to_json(v.normal, 1), "d": v.d}
		TYPE_QUATERNION: return {"x": v.x, "y": v.y, "z": v.z, "w": v.w}
		TYPE_AABB: return {"position": _variant_to_json(v.position, 1), "size": _variant_to_json(v.size, 1)}
		TYPE_BASIS: return {"x": _variant_to_json(v.x, 1), "y": _variant_to_json(v.y, 1), "z": _variant_to_json(v.z, 1)}
		TYPE_TRANSFORM3D: return {"basis": _variant_to_json(v.basis, 1), "origin": _variant_to_json(v.origin, 1)}
		TYPE_PROJECTION: return {"x": _variant_to_json(v.x, 1), "y": _variant_to_json(v.y, 1), "z": _variant_to_json(v.z, 1), "w": _variant_to_json(v.w, 1)}
		_: return str(v)

func _get_editor_errors(id: Variant, params: Dictionary = {}) -> Dictionary:
	# Errores reales del juego en ejecución, capturados vía señal debug_data
	# (RF-09, vault 18 §4). Sin juego corriendo, vacío.
	var errs: Array = []
	for e in _debug_errors:
		errs.append({
			"file": e.get("file", ""),
			"func": e.get("func", ""),
			"line": e.get("line", 0),
			"message": e.get("message", ""),
			"descr": e.get("descr", ""),
			"warning": e.get("warning", false),
		})
	return _ok(id, {"errors": errs, "running": _editor.is_playing_scene()})

func _resolve_node(params: Dictionary) -> Node:
	var root: Node = _editor.get_edited_scene_root()
	var raw: String = params.get("path", ".")
	var node_path: String = _validate_path(raw)
	if node_path == "":
		return null
	return root if node_path == "." else root.get_node_or_null(NodePath(node_path))

func _call_method(id: Variant, params: Dictionary) -> Dictionary:
	var target: Object = _resolve_object(params)
	if not target:
		return _error(id, "godot", "nodo o recurso no encontrado (usa path o object)")
	var method: String = params.get("method", "")
	if method == "":
		return _error(id, "validation", "parámetro 'method' requerido")
	var raw_args: Array = params.get("args", [])
	var args: Array = []
	for a in raw_args:
		var v: Variant = _json_to_variant(a)
		# $res: cargar un recurso res:// desde el dict (útil para args como
		# add_frame(anim, texture) donde texture es un Resource).
		if typeof(v) == TYPE_DICTIONARY and v.has("$res"):
			v = load(str(v.get("$res", "")))
		args.append(v)
	var result: Variant
	if args.size() == 0:
		result = target.call(method)
	else:
		result = target.callv(method, args)
	return _ok(id, {
		"target": _target_label(params),
		"method": method,
		"result": _variant_to_json(result),
	})

func _object_property_get(id: Variant, params: Dictionary) -> Dictionary:
	var target: Object = _resolve_object(params)
	if not target:
		return _error(id, "godot", "nodo o recurso no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "parámetro 'property' requerido")
	var value: Variant = target.get(prop)
	return _ok(id, {"target": _target_label(params), "property": prop, "value": _variant_to_json(value)})

func _object_property_set(id: Variant, params: Dictionary) -> Dictionary:
	var target: Object = _resolve_object(params)
	if not target:
		return _error(id, "godot", "nodo o recurso no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "parámetro 'property' requerido")
	var value: Variant = params.get("value")
	var old_value: Variant = target.get(prop)
	if target is Node:
		target.set(prop, _json_to_variant(value))
		_undo_redo.create_action("Osiris: %s" % prop)
		_undo_redo.add_do_method(target, "set", prop, _json_to_variant(value))
		_undo_redo.add_undo_method(target, "set", prop, old_value)
		_undo_redo.commit_action()
	else:
		target.set(prop, _json_to_variant(value))
	return _ok(id, {"target": _target_label(params), "property": prop, "value": _variant_to_json(target.get(prop))})

## Resuelve el objeto destino: nodo por `path`, o recurso por `object` (res://).
func _resolve_object(params: Dictionary) -> Object:
	if params.has("object"):
		var res_path: String = params.get("object", "")
		if res_path != "":
			return load(res_path)
	return _resolve_node(params)

func _target_label(params: Dictionary) -> String:
	if params.has("object"):
		return params.get("object", "")
	return params.get("path", ".")

func _scene_apply(id: Variant, params: Dictionary) -> Dictionary:
	var operations: Array = params.get("operations", [])
	if operations.size() == 0:
		return _error(id, "validation", "parámetro 'operations' requerido (array)")
	var receipts: Array = []
	for i in operations.size():
		var op: Dictionary = operations[i] if typeof(operations[i]) == TYPE_DICTIONARY else {}
		var kind: String = op.get("operation", "")
		match kind:
			"create_node":
				var r := _create_node(id, {"path": op.get("parent", "."), "type": op.get("type", ""), "name": op.get("name", "")})
				receipts.append({"step": i, "operation": kind, "result": r.get("ok", r.get("error", {}))})
			"remove_node":
				var r := _remove_node(id, {"path": op.get("path", "")})
				receipts.append({"step": i, "operation": kind, "result": r.get("ok", r.get("error", {}))})
			"move_node":
				var r := _move_node(id, {"path": op.get("path", ""), "new_parent": op.get("new_parent", ""), "new_name": op.get("new_name", "")})
				receipts.append({"step": i, "operation": kind, "result": r.get("ok", r.get("error", {}))})
			"set_property":
				var r := _set_property(id, {"path": op.get("path", "."), "property": op.get("property", ""), "value": op.get("value")})
				receipts.append({"step": i, "operation": kind, "result": r.get("ok", r.get("error", {}))})
			"instance_scene":
				var r := _instance_scene(id, {"path": op.get("parent", "."), "scene": op.get("scene", ""), "name": op.get("name", "")})
				receipts.append({"step": i, "operation": kind, "result": r.get("ok", r.get("error", {}))})
			_:
				receipts.append({"step": i, "operation": kind, "result": {"error": "operación desconocida: %s" % kind}})
	return _ok(id, {"receipts": receipts})

## Mueve un nodo a otro padre (reparent), con Undo/Redo y nombre opcional.
func _move_node(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	if node == _editor.get_edited_scene_root():
		return _error(id, "godot", "no se puede mover la raíz de la escena")
	var new_parent_raw: String = params.get("new_parent", "")
	if new_parent_raw == "":
		return _error(id, "validation", "parámetro 'new_parent' requerido")
	var new_parent: Node = _resolve_path_node(new_parent_raw)
	if not new_parent:
		return _error(id, "godot", "nuevo padre no encontrado")
	if new_parent.is_ancestor_of(node):
		return _error(id, "godot", "no se puede mover un nodo a un descendiente suyo")
	var old_parent: Node = node.get_parent()
	var old_name: String = node.name
	var new_name: String = params.get("new_name", "")
	_undo_redo.create_action("Osiris: mover nodo")
	_undo_redo.add_do_method(old_parent, "remove_child", node)
	_undo_redo.add_do_method(new_parent, "add_child", node, true)
	if new_name != "":
		_undo_redo.add_do_method(node, "set_name", new_name)
	_undo_redo.add_undo_method(new_parent, "remove_child", node)
	_undo_redo.add_undo_method(old_parent, "add_child", node, true)
	_undo_redo.add_undo_method(node, "set_name", old_name)
	_undo_redo.commit_action()
	return _ok(id, {"path": params.get("path", "."), "new_parent": new_parent_raw, "name": new_name if new_name != "" else node.name})

## Instancia una escena empaquetada (res://*.tscn) como hijo de un nodo, con
## Undo/Redo. Patrón del editor (scene_tree_dock.cpp:370).
func _instance_scene(id: Variant, params: Dictionary) -> Dictionary:
	var parent: Node = _resolve_node(params)
	if not parent:
		return _error(id, "godot", "nodo padre no encontrado")
	var scene_path: String = params.get("scene", "")
	if scene_path == "":
		return _error(id, "validation", "parámetro 'scene' requerido")
	if not scene_path.begins_with("res://"):
		return _error(id, "validation", "escena debe comenzar con res://")
	var packed: PackedScene = load(scene_path)
	if not packed:
		return _error(id, "godot", "no se pudo cargar la escena: %s" % scene_path)
	var instance: Node = packed.instantiate()
	var node_name: String = params.get("name", "")
	instance.name = node_name if node_name != "" else instance.name
	var root: Node = _editor.get_edited_scene_root()
	_undo_redo.create_action("Osiris: instanciar escena")
	_undo_redo.add_do_method(parent, "add_child", instance, true)
	_undo_redo.add_do_method(instance, "set_owner", root)
	_undo_redo.add_do_reference(instance)
	_undo_redo.add_undo_method(parent, "remove_child", instance)
	_undo_redo.commit_action()
	return _ok(id, {"path": "%s/%s" % [params.get("path", "."), instance.name], "scene": scene_path, "name": instance.name})

## Resuelve un NodePath relativo al root (mismo saneo que _validate_path).
func _resolve_path_node(raw: String) -> Node:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return null
	var node_path: String = _validate_path(raw)
	if node_path == "":
		return null
	return root if node_path == "." else root.get_node_or_null(NodePath(node_path))

func _create_node(id: Variant, params: Dictionary) -> Dictionary:
	var parent: Node = _resolve_node(params)
	if not parent:
		return _error(id, "godot", "nodo padre no encontrado")
	var raw_path: String = params.get("path", ".")
	var node_type: String = params.get("type", "")
	if node_type == "":
		return _error(id, "validation", "parámetro 'type' requerido")
	var new_node: Variant = ClassDB.instantiate(node_type)
	if not new_node:
		return _error(id, "godot", "tipo de nodo desconocido: %s" % node_type)
	var node_name: String = params.get("name", "")
	new_node.name = node_name if node_name else node_type
	parent.add_child(new_node, true)
	new_node.set_owner(_editor.get_edited_scene_root())
	_undo_redo.create_action("Osiris: crear nodo")
	_undo_redo.add_do_method(new_node, "set_owner", _editor.get_edited_scene_root())
	_undo_redo.add_undo_method(parent, "remove_child", new_node)
	_undo_redo.commit_action()
	return _ok(id, {"path": "%s/%s" % [raw_path, new_node.name], "type": node_type, "name": new_node.name})

## Crea un Tween en un nodo (Node.create_tween, bound). Para animaciones.
func _create_tween(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var tween := node.create_tween()
	return _ok(id, {"path": params.get("path", "."), "tween": str(tween), "note": "usa object.call sobre el Tween: tween_property, tween_method, etc."})

## Asigna una descripción en el editor (set_editor_description, bound).
func _set_description(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	node.set_editor_description(params.get("description", ""))
	return _ok(id, {"path": params.get("path", "."), "description": node.get_editor_description()})

## Marca un nodo con acceso % (set_unique_name_in_owner, bound).
func _set_unique_name(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var enable: bool = params.get("enable", true)
	node.set_unique_name_in_owner(enable)
	return _ok(id, {"path": params.get("path", "."), "unique_name": enable})

## Redpadrea un nodo (Node.reparent, bound). A diferencia de move_node, usa
## la API nativa que preserva la transformación global.
func _reparent_node(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	if node == _editor.get_edited_scene_root():
		return _error(id, "godot", "no se puede reparentear la raíz")
	var new_parent_raw: String = params.get("new_parent", "")
	if new_parent_raw == "":
		return _error(id, "validation", "parámetro 'new_parent' requerido")
	var new_parent: Node = _resolve_path_node(new_parent_raw)
	if not new_parent:
		return _error(id, "godot", "nuevo padre no encontrado")
	var keep_transform: bool = params.get("keep_transform", true)
	node.reparent(new_parent, keep_transform)
	return _ok(id, {"path": params.get("path", "."), "new_parent": new_parent_raw, "keep_transform": keep_transform})

## Duplica un nodo (Node.duplicate profundo) como hermano, con Undo/Redo.
func _duplicate_node(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	if node == _editor.get_edited_scene_root():
		return _error(id, "godot", "no se puede duplicar la raíz de la escena")
	var parent: Node = node.get_parent()
	var dup: Node = node.duplicate()
	var node_name: String = params.get("name", "")
	dup.name = node_name if node_name != "" else node.name
	var root: Node = _editor.get_edited_scene_root()
	_undo_redo.create_action("Osiris: duplicar nodo")
	_undo_redo.add_do_method(parent, "add_child", dup, true)
	_undo_redo.add_do_method(dup, "set_owner", root)
	_undo_redo.add_do_reference(dup)
	_undo_redo.add_undo_method(parent, "remove_child", dup)
	_undo_redo.commit_action()
	return _ok(id, {"path": "%s/%s" % [params.get("path", "."), dup.name], "name": dup.name})

## Registra una acción de InputMap (InputMap.add_action, input_map.cpp:44) y la
## persiste en project.godot bajo `input/<accion>` (project_settings.cpp:1676).
func _inputmap_add(id: Variant, params: Dictionary) -> Dictionary:
	var action: String = params.get("action", "")
	if action == "":
		return _error(id, "validation", "parámetro 'action' requerido")
	if InputMap.has_action(action):
		return _error(id, "conflict", "la acción ya existe: %s" % action)
	InputMap.add_action(action)
	# Formato 4.7: dict con deadzone + events (project_settings.cpp:626). El array
	# vacío es formato legacy y dispara warning al cargar project.godot.
	ProjectSettings.set_setting("input/%s" % action, {"deadzone": 0.5, "events": []})
	var err: int = ProjectSettings.save()
	if err != OK:
		return _error(id, "godot", "no se pudo guardar project.godot (código %d)" % err)
	return _ok(id, {"action": action, "saved": true})

func _remove_node(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	if node == _editor.get_edited_scene_root():
		return _error(id, "godot", "no se puede eliminar la raíz de la escena")
	var node_name: String = node.name
	var parent: Node = node.get_parent()
	var index: int = node.get_index()
	# Solo vía undo_redo (patrón _set_property): el commit ejecuta el do_method
	# que elimina. Un cambio directo + do_method duplicaría (remove_child: parent != this).
	_undo_redo.create_action("Osiris: eliminar nodo")
	_undo_redo.add_do_method(parent, "remove_child", node)
	_undo_redo.add_do_reference(node)
	_undo_redo.add_undo_method(parent, "add_child", node, true)
	_undo_redo.add_undo_method(parent, "move_child", node, index)
	_undo_redo.commit_action()
	return _ok(id, {"removed": node_name})

func _set_property(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "parámetro 'property' requerido")
	var value: Variant = params.get("value")
	var old_value: Variant = node.get(prop)
	node.set(prop, _json_to_variant(value))
	_undo_redo.create_action("Osiris: cambiar %s" % prop)
	_undo_redo.add_do_method(node, "set", prop, _json_to_variant(value))
	_undo_redo.add_undo_method(node, "set", prop, old_value)
	_undo_redo.commit_action()
	return _ok(id, {"property": prop, "value": _variant_to_json(node.get(prop))})

func _json_to_variant(v: Variant) -> Variant:
	# Coercer por heurística: dicts con keys {x,y} → Vector2, {x,y,z} → Vector3,
	# {x,y,z,w} → Quaternion. Riesgo: un dict no-vector con esas keys se coercerá
	# incorrectamente a vector. Si necesitas forzar tipo, pasa type_hint en params (FND-009).
	if v == null: return null
	var t: int = typeof(v)
	if t == TYPE_BOOL or t == TYPE_INT or t == TYPE_FLOAT or t == TYPE_STRING:
		return v
	if t == TYPE_DICTIONARY:
		# Intentar reconstruir un vector, color, etc. desde keys
		if v.has("x") and v.has("y") and v.size() <= 4:
			var keys: Array = v.keys()
			if keys.size() == 3 and v.has("z"):
				return Vector3(v.x, v.y, v.z)
			elif keys.size() == 4 and v.has("z") and v.has("w"):
				return Quaternion(v.x, v.y, v.z, v.w)
			elif keys.size() == 2:
				return Vector2(v.x, v.y)
		return v  # dict sin tipo, se pasa tal cual (Godot lo convierte)
	if t == TYPE_ARRAY:
		var arr: Array = []
		for item: Variant in v:
			arr.append(_json_to_variant(item))
		return arr
	return v

func _save_scene(id: Variant, params: Dictionary = {}) -> Dictionary:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _error(id, "godot", "no hay escena activa")
	var path: String = root.scene_file_path
	if path == "":
		return _error(id, "godot", "la escena no tiene archivo asociado")
	var packed := PackedScene.new()
	var ok: int = packed.pack(root)
	if ok != OK:
		return _error(id, "godot", "PackedScene.pack devolvio %d" % ok)
	ok = ResourceSaver.save(packed, path, ResourceSaver.FLAG_REPLACE_SUBRESOURCE_PATHS)
	if ok != OK:
		return _error(id, "godot", "ResourceSaver.save devolvio %d" % ok)
	return _ok(id, {"saved": path})

## Guarda la escena activa con un nuevo path (save_scene_as, bound).
func _save_scene_as(id: Variant, params: Dictionary) -> Dictionary:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _error(id, "godot", "no hay escena activa")
	var path: String = params.get("path", "")
	if path == "" or not path.begins_with("res://"):
		return _error(id, "validation", "parámetro 'path' requerido (res://*.tscn)")
	_editor.save_scene_as(path)
	return _ok(id, {"saved": path})

## Guarda todas las escenas abiertas (save_all_scenes, bound).
func _save_all_scenes(id: Variant, params: Dictionary = {}) -> Dictionary:
	_editor.save_all_scenes()
	return _ok(id, {"saved": true})

## Lista las escenas con cambios sin guardar (get_unsaved_scenes, bound).
func _unsaved_scenes(id: Variant, params: Dictionary = {}) -> Dictionary:
	var scenes: Array = []
	for s in _editor.get_unsaved_scenes():
		scenes.append(String(s))
	return _ok(id, {"unsaved": scenes})

## Foco del inspector en un nodo (inspect_object, bound). Si no se pasa path,
## limpia el inspector. Con path, selecciona y abre sus propiedades.
func _inspect_node(id: Variant, params: Dictionary = {}) -> Dictionary:
	var path: String = params.get("path", "")
	if path != "":
		var node: Node = _resolve_node(params)
		if not node:
			return _error(id, "godot", "nodo no encontrado")
		_editor.inspect_object(node)
		return _ok(id, {"inspected": path})
	_editor.inspect_object(null)
	return _ok(id, {"inspected": null, "note": "inspector limpiado"})
	return _ok(id, {"saved": path})

## Lista las escenas abiertas en el editor (EditorInterface.get_open_scenes).
func _open_scenes(id: Variant, params: Dictionary = {}) -> Dictionary:
	var scenes: Array = []
	for s in _editor.get_open_scenes():
		scenes.append(String(s))
	return _ok(id, {"scenes": scenes})

## Abre una escena (res://*.tscn) en el editor. La carga la hace el propio
## editor (`EditorNode::open_scene`, editor_node.cpp:5042) — no pide confirmar
## si hay cambios sin guardar en la escena actual: el llamador es responsable
## de guardar antes (godot.scene.save).
func _open_scene(id: Variant, params: Dictionary) -> Dictionary:
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido")
	if not path.begins_with("res://") or path.get_extension() != "tscn":
		return _error(id, "validation", "ruta debe ser res://*.tscn")
	if not FileAccess.file_exists(path):
		return _error(id, "godot", "la escena no existe: %s" % path)
	var root: Node = _editor.get_edited_scene_root()
	if root and root.scene_file_path != path and _has_unsaved_scenes():
		return _error(id, "conflict", "hay escenas sin guardar; guarda primero (godot.scene.save)")
	_editor.open_scene_from_path(path)
	return _ok(id, {"opened": path})

func _has_unsaved_scenes() -> bool:
	return _editor.get_unsaved_scenes().size() > 0

## Crea una escena nueva (res://*.tscn): construye el nodo raíz, lo empaqueta
## como PackedScene, lo guarda a disco y lo abre en el editor. No depende de
## `EditorNode::new_scene()` (no expuesto a GDScript): la escena se crea y se
## abre como cualquier otra.
func _create_scene(id: Variant, params: Dictionary) -> Dictionary:
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido (res://*.tscn)")
	if not path.begins_with("res://") or path.get_extension() != "tscn":
		return _error(id, "validation", "ruta debe ser res://*.tscn")
	if FileAccess.file_exists(path):
		return _error(id, "conflict", "la escena ya existe: %s (usa godot.scene.open)" % path)
	if _has_unsaved_scenes():
		return _error(id, "conflict", "hay escenas sin guardar; guarda primero (godot.scene.save)")
	var node_type: String = params.get("type", "Node2D")
	var root: Variant = ClassDB.instantiate(node_type)
	if not root:
		return _error(id, "godot", "tipo de nodo desconocido: %s" % node_type)
	root.name = params.get("name", node_type)
	var packed := PackedScene.new()
	var err: int = packed.pack(root)
	if err != OK:
		return _error(id, "godot", "PackedScene.pack devolvió %d" % err)
	err = ResourceSaver.save(packed, path, ResourceSaver.FLAG_REPLACE_SUBRESOURCE_PATHS)
	if err != OK:
		return _error(id, "godot", "ResourceSaver.save devolvió %d" % err)
	_editor.open_scene_from_path(path)
	return _ok(id, {"created": path, "root": root.name, "type": node_type})

## Selecciona nodos en el árbol del editor (EditorSelection.add_node).
func _select_nodes(id: Variant, params: Dictionary) -> Dictionary:
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _error(id, "godot", "no hay escena activa")
	var paths: Array = params.get("paths", [])
	if typeof(paths) == TYPE_ARRAY and paths.size() > 0:
		var sel: EditorSelection = _editor.get_selection()
		sel.clear()
		for raw in paths:
			var node_path: String = _validate_path(str(raw))
			if node_path == "":
				return _error(id, "validation", "ruta inválida: %s" % str(raw))
			var node: Node = root if node_path == "." else root.get_node_or_null(NodePath(node_path))
			if node:
				sel.add_node(node)
	# Siempre devuelve la selección actual (útil para leer sin modificar).
	var current: Array = []
	for n in _editor.get_selection().get_selected_nodes():
		current.append({"name": n.name, "class": n.get_class()})
	return _ok(id, {"selected": paths, "selection": current})

## (Interna) lee la selección actual del editor.
func _get_selection(id: Variant, params: Dictionary = {}) -> Dictionary:
	var sel: EditorSelection = _editor.get_selection()
	var nodes: Array = []
	for n in sel.get_selected_nodes():
		nodes.append({"name": n.name, "class": n.get_class()})
	return _ok(id, {"selection": nodes})

## Screenshot del viewport del editor (2D/3D), no del juego. Usa
## `SubViewport.get_texture().get_image().save_png()` (editor_interface.cpp:347).
func _editor_screenshot(id: Variant, params: Dictionary = {}) -> Dictionary:
	var viewport: SubViewport = null
	var view3d: bool = bool(params.get("view3d", false))
	if view3d:
		viewport = _editor.get_editor_viewport_3d(0)
	else:
		viewport = _editor.get_editor_viewport_2d()
	if not viewport:
		return _error(id, "godot", "no se pudo obtener el viewport del editor")
	var img: Image = viewport.get_texture().get_image()
	if not img or img.get_width() <= 0:
		return _error(id, "godot", "el viewport del editor aún no renderiza")
	var out_path: String = params.get("output", "user://osiris_editor_shot.png")
	if not out_path.begins_with("user://") and not out_path.begins_with("res://"):
		return _error(id, "validation", "output debe ser user:// o res://")
	var err: int = img.save_png(out_path)
	if err != OK:
		return _error(id, "godot", "no se pudo guardar el PNG (código %d)" % err)
	return _ok(id, {"path": out_path, "view": "3d" if view3d else "2d", "size": "%dx%d" % [img.get_width(), img.get_height()]})

## Registra un autoload singleton. Se guarda en project.godot como
## `autoload/Name="*res://script.gd"` (core/config/project_settings.cpp:359);
## el editor lo instancia al siguiente arranque. Requiere perfiles write.
func _add_autoload(id: Variant, params: Dictionary) -> Dictionary:
	var name: String = params.get("name", "")
	if name == "":
		return _error(id, "validation", "parámetro 'name' requerido")
	var path: String = params.get("path", "")
	if path == "":
		return _error(id, "validation", "parámetro 'path' requerido (res://*.gd)")
	if not path.begins_with("res://") or path.get_extension() != "gd":
		return _error(id, "validation", "path debe ser res://*.gd")
	if not FileAccess.file_exists(path):
		return _error(id, "godot", "el script no existe: %s" % path)
	ProjectSettings.set_setting("autoload/%s" % name, "*%s" % path)
	var err: int = ProjectSettings.save()
	if err != OK:
		return _error(id, "godot", "no se pudo guardar project.godot (código %d)" % err)
	return _ok(id, {"autoload": name, "path": path, "saved": true})

## Exporta el proyecto (release/debug) con un preset de export_presets.cfg.
## Godot solo exporta vía CLI (`--export-release`), patrón main.cpp:4064; en el
## editor el singleton EditorExport no expone métodos a GDScript, así que
## lanzamos un subproceso headless igual que _validate_script (OS.execute).
func _export_project(id: Variant, params: Dictionary) -> Dictionary:
	var preset: String = params.get("preset", "")
	if preset == "":
		return _error(id, "validation", "parámetro 'preset' requerido (nombre en export_presets.cfg)")
	var output: String = params.get("output", "")
	if output == "":
		return _error(id, "validation", "parámetro 'output' requerido (ruta de salida)")
	var debug: bool = bool(params.get("debug", false))
	# Godot resuelve la salida relativa al proyecto.
	var args: Array = [
		"--headless",
		"--path", ProjectSettings.globalize_path("res://"),
		"--export-release" if not debug else "--export-debug",
		preset,
		output,
	]
	var out: Array = []
	var exit_code: int = OS.execute(OS.get_executable_path(), args, out, true)
	if exit_code != 0:
		var stderr: String = ""
		for o in out:
			if typeof(o) == TYPE_STRING:
				stderr += str(o)
		return _error(id, "godot", "export falló (código %d): %s" % [exit_code, _sanitize(stderr)])
	return _ok(id, {"exported": output, "preset": preset, "debug": debug})

## El stderr de un subproceso Godot trae códigos ANSI (colores) y caracteres de
## control; el contrato NDJSON es UTF-8 estricto y serde_json rechaza \u001B.
func _sanitize(s: String) -> String:
	var clean: String = s
	var ansi := RegEx.create_from_string("\u001b\\[[0-9;]*m")
	clean = ansi.sub(clean, "", true)
	var ctl := RegEx.create_from_string("[\\x00-\\x08\\x0B\\x0C\\x0E-\\x1F]")
	clean = ctl.sub(clean, "", true)
	return clean.strip_edges()

func _validate_path(raw: String) -> String:
	if raw == "" or raw == ".":
		return "."
	# `res://` es el prefijo legítimo de recursos; validar el resto sin `//`
	# intermedio, manteniendo el bloqueo de traversal `..`.
	var rest: String = raw
	if raw.begins_with("res://"):
		rest = raw.substr("res://".length())
	elif raw.begins_with("/") or raw.contains(".."):
		return ""
	if rest.contains("..") or rest.contains("//"):
		return ""
	return raw

func _run_project(id: Variant, params: Dictionary) -> Dictionary:
	# El editor lanza el juego con --remote-debug y arranca el debugger server
	# (editor_run_bar.cpp: _run_scene). El EditorDebuggerPlugin captura
	# errors/output. Sin OS.create_process: un solo proceso, con debugger.
	var scene: String = params.get("scene", "")
	if scene != "":
		var valid_scene: String = _validate_path(scene)
		if valid_scene == "":
			return _error(id, "validation", "ruta de escena inválida: %s" % scene)
		_editor.play_custom_scene(valid_scene)
	else:
		_editor.play_main_scene()
	if _debug_errors:
		_debug_errors.clear()
	if _debug_output:
		_debug_output.clear()
	_debug_tree.clear()
	_debug_metric_names.clear()
	_debug_metrics.clear()
	# Timeout real en segundos, revisado en poll() (no frames).
	var timeout_secs: int = int(params.get("timeout_secs", 0))
	_run_deadline_ms = Time.get_ticks_msec() + timeout_secs * 1000 if timeout_secs > 0 else 0
	# Historial de ejecución (MVP5): marca inicio.
	_run_start_ms = Time.get_ticks_msec()
	_run_started_scene = scene if scene != "" else "main"
	return _ok(id, {"running": true, "scene": scene if scene != "" else "main", "pid": 0})

const OSIRIS_INPUT_FILE := "user://osiris_input.json"
const OSIRIS_INPUT_AUTOLOAD := "res://addons/osiris-bridge/osiris_input_autoload.gd"
const OSIRIS_INPUT_AUTOLOAD_NAME := "OsirisInput"
var _input_seq: int = 0

## Inyecta input en el juego en ejecución escribiendo a user://osiris_input.json.
## Un autoload del juego (osiris_input_autoload.gd) lee el archivo en
## _physics_process y llama Input.parse_input_event o Input.action_press.
## Sin debugger, sin Live Edit: simple y robusto.
func _input_send(id: Variant, params: Dictionary) -> Dictionary:
	if not _editor.is_playing_scene():
		return _error(id, "godot", "no hay juego en ejecución (godot.run)")
	var input: Variant = params.get("input")
	if typeof(input) != TYPE_DICTIONARY:
		return _error(id, "validation", "parámetro 'input' requerido (dict: key/action + pressed)")
	if not input.has("key") and not input.has("action"):
		return _error(id, "validation", "input debe tener 'key' (ej: W) o 'action' (ej: jump)")
	# Un entry por archivo con seq monótono: el autoload solo procesa seq > _last_seq,
	# evitando carreras entre el bridge (editor) y el autoload (juego).
	_input_seq += 1
	var entry: Dictionary = {"seq": _input_seq, "pressed": bool(input.get("pressed", false))}
	if input.has("key"):
		entry["key"] = str(input.get("key", ""))
	else:
		entry["action"] = str(input.get("action", ""))
	var w := FileAccess.open(OSIRIS_INPUT_FILE, FileAccess.WRITE)
	if not w:
		return _error(id, "godot", "no se pudo escribir %s" % OSIRIS_INPUT_FILE)
	w.store_string(JSON.stringify(entry))
	w.close()
	return _ok(id, {"sent": true, "input": input, "method": "file"})

func _stop_project(id: Variant, params: Dictionary = {}) -> Dictionary:
	_run_deadline_ms = 0
	_editor.stop_playing_scene()
	_record_run_stop("stopped")
	return _ok(id, {"running": false})

## Recarga silenciosamente las escenas modificadas (evita el diálogo "files
## changed, reload?"). EditorInterface.reload_scene_from_path (bound).
func _reload_editor(id: Variant, params: Dictionary = {}) -> Dictionary:
	# Guardar antes de recargar: evita perder cambios y crash por estado sucio.
	_editor.save_scene()
	var reloaded: Array = []
	for scene_path in _editor.get_open_scenes():
		_editor.reload_scene_from_path(String(scene_path))
		reloaded.append(String(scene_path))
	_editor.get_resource_filesystem().scan()
	return _ok(id, {"reloaded": reloaded, "saved": true})

func _get_run_status(id: Variant, params: Dictionary = {}) -> Dictionary:
	var running: bool = _editor.is_playing_scene()
	if not running:
		_run_deadline_ms = 0
		if _run_start_ms > 0:
			_record_run_stop("finished")
	return _ok(id, {"running": running})

## Registra el fin de una ejecución en el historial JSONL (MVP5).
func _record_run_stop(reason: String) -> void:
	var duration_ms: int = Time.get_ticks_msec() - _run_start_ms
	var output: Array = _debug_output
	var tail: Array = output.slice(maxi(output.size() - 30, 0))
	var errors: Array = []
	for e in _debug_errors:
		errors.append({
			"file": e.get("file", ""),
			"line": e.get("line", 0),
			"message": e.get("message", ""),
			"warning": e.get("warning", false),
		})
	var entry := {
		"started_at": Time.get_datetime_string_from_system(),
		"scene": _run_started_scene,
		"duration_ms": duration_ms,
		"reason": reason,
		"output_tail": "\n".join(tail),
		"error_count": errors.size(),
		"errors": errors,
	}
	# Append JSONL: leer lo existente (si hay), añadir la línea, reescribir.
	# `user://` no siempre permite READ_WRITE en headless; WRITE es fiable.
	var lines: Array = []
	var r: FileAccess = FileAccess.open(RUN_HISTORY_FILE, FileAccess.READ)
	if r:
		while not r.eof_reached():
			var line: String = r.get_line()
			if line.strip_edges() != "":
				lines.append(line)
		r.close()
	lines.append(JSON.stringify(entry))
	if lines.size() > RUN_HISTORY_MAX:
		lines = lines.slice(lines.size() - RUN_HISTORY_MAX)
	var w: FileAccess = FileAccess.open(RUN_HISTORY_FILE, FileAccess.WRITE)
	if w:
		for l in lines:
			w.store_line(l)
		w.close()
		_run_start_ms = 0
		_run_started_scene = ""
	else:
		push_error("[OsirisBridge] no se pudo escribir el historial: %s" % RUN_HISTORY_FILE)
		_run_start_ms = 0
		_run_started_scene = ""

## Lee el historial de ejecuciones (MVP5): últimos N runs, o los de una escena.
func _run_history(id: Variant, params: Dictionary) -> Dictionary:
	var max_entries: int = clampi(params.get("max_entries", 50), 1, RUN_HISTORY_MAX)
	var scene_filter: String = params.get("scene", "")
	var entries: Array = []
	var f: FileAccess = FileAccess.open(RUN_HISTORY_FILE, FileAccess.READ)
	if f:
		var lines: Array = []
		while not f.eof_reached():
			var line: String = f.get_line()
			if line.strip_edges() != "":
				lines.append(line)
		f.close()
		var start: int = maxi(lines.size() - max_entries, 0)
		for i in range(start, lines.size()):
			var json := JSON.new()
			if json.parse(lines[i]) == OK:
				var entry: Dictionary = json.get_data()
				if scene_filter == "" or entry.get("scene", "") == scene_filter:
					entries.append(entry)
	return _ok(id, {"entries": entries, "total": entries.size()})

func _get_run_output(id: Variant, params: Dictionary = {}) -> Dictionary:
	var lines: Array = _debug_output
	var tail: Array = lines.slice(maxi(lines.size() - 200, 0))
	return _ok(id, {"output": "\n".join(tail), "running": _editor.is_playing_scene()})

func _runtime_tree(id: Variant, params: Dictionary = {}) -> Dictionary:
	# Árbol remoto del juego (flat DFS) capturado vía debug_data. Sin árbol,
	# devolver el de la escena activa del editor como fallback.
	if _debug_tree.size() > 0:
		return _ok(id, {"tree": _debug_tree, "source": "runtime"})
	var root: Node = _editor.get_edited_scene_root()
	if not root:
		return _ok(id, {"tree": [], "source": "none"})
	var items: Array = []
	_collect_runtime_nodes(root, items)
	return _ok(id, {"tree": items, "source": "editor"})

func _collect_runtime_nodes(node: Node, items: Array) -> void:
	for child: Node in node.get_children():
		items.append({
			"child_count": child.get_child_count(),
			"name": child.name,
			"type": child.get_class(),
			"id": child.get_instance_id(),
			"scene_file": child.scene_file_path,
		})
		_collect_runtime_nodes(child, items)

func _handle_debug_inspect(id: Variant, params: Dictionary) -> Dictionary:
	var target_id: int = int(params.get("id", 0))
	var path: String = params.get("path", "")
	if target_id == 0 and path != "":
		target_id = _tree_id_for_path(path)
	if target_id == 0:
		return _error(id, "validation", "parámetro 'id' o 'path' requerido (id del árbol remoto)")
	if _controller:
		_controller.request_inspect([target_id])
		# La respuesta llega por debug_data en el siguiente poll; devolvemos lo
		# último capturado para ese id.
	var items: Array = []
	for obj in _debug_inspect:
		if typeof(obj) != TYPE_ARRAY or obj.size() < 1:
			continue
		if int(obj[0]) == target_id:
			items.append(_inspect_obj_to_json(obj))
	if items.size() == 0:
		return _ok(id, {"id": target_id, "inspect": [], "note": "petición enviada; consulta de nuevo si aún no llegó"})
	return _ok(id, {"id": target_id, "inspect": items})

func _tree_id_for_path(path: String) -> int:
	# El árbol remoto no guarda paths; resolvemos por nombre del nodo raíz del
	# árbol capturado (el id del primer nodo cuyo nombre coincide).
	for node in _debug_tree:
		if node.get("name", "") == path or node.get("name", "") == path.get_file():
			return int(node.get("id", 0))
	return 0

## Convierte un objeto inspeccionado `[id, class_name, props]` a JSON legible.
func _inspect_obj_to_json(obj: Array) -> Dictionary:
	var props: Dictionary = {}
	if obj.size() >= 3 and typeof(obj[2]) == TYPE_ARRAY:
		for p in obj[2]:
			if typeof(p) == TYPE_ARRAY and p.size() >= 6:
				props[p[0]] = _variant_to_json(p[5])
	return {
		"id": obj[0],
		"class": obj[1],
		"properties": props,
	}

func _runtime_capture(id: Variant, params: Dictionary = {}) -> Dictionary:
	# Screenshot del juego (MVP4): pide al juego capturar y devuelve la ruta del
	# PNG que ya guardó en temp (vía game_view:get_screenshot).
	if not _editor.is_playing_scene():
		return _error(id, "godot", "no hay juego en ejecución")
	if _controller:
		_controller.request_screenshot()
	# La respuesta llega por debug_data en el siguiente poll; si aún no llegó,
	# devolver la última ruta capturada (o vacía la primera vez).
	return _ok(id, {"path": _debug_screenshot_path, "running": true, "note": "consulta de nuevo si la ruta está vacía (el juego tarda un frame)"})

func _runtime_metrics(id: Variant, params: Dictionary = {}) -> Dictionary:
	# Monitores del juego (performance:profile_frame). Los nombres estándar de
	# Godot son fijos y conocidos; los custom llegan vía profile_names.
	var metrics: Array = []
	var n: int = _debug_metrics.size()
	for i in n:
		var name: String = ""
		if i < _debug_metric_names.size():
			name = _debug_metric_names[i]
		elif i < STANDARD_MONITORS.size():
			name = STANDARD_MONITORS[i]
		else:
			name = "custom_%d" % i
		metrics.append({
			"name": name,
			"value": _debug_metrics[i],
		})
	return _ok(id, {"metrics": metrics, "running": _editor.is_playing_scene()})

func _assert_property(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var prop: String = params.get("property", "")
	if prop == "":
		return _error(id, "validation", "propiedad requerida")
	var actual: Variant = node.get(prop)
	var expected: Variant = params.get("expected")
	var match_val: bool = str(actual) == str(expected)
	return _ok(id, {"match": match_val, "actual": _variant_to_json(actual), "expected": expected})

func _generate_token() -> String:
	var bytes: PackedByteArray = PackedByteArray()
	bytes.resize(32)
	for i in 32:
		bytes[i] = randi() % 256
	return bytes.hex_encode()

func _write_token() -> void:
	var f: FileAccess = FileAccess.open(TOKEN_FILE, FileAccess.WRITE)
	if f:
		f.store_string(_token)
		f.close()
	# Ruta estable por proyecto (no depende del nombre, que cambia user://).
	# El wrapper de opencode la lee para conectar el MCP server al bridge.
	var stable: FileAccess = FileAccess.open("res://.godot/osiris_ipc_token", FileAccess.WRITE)
	if stable:
		stable.store_string(_token)
		stable.close()
