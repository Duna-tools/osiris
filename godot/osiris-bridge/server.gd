@tool
class_name OsirisBridgeServer
extends RefCounted

const PROTOCOL_VERSION := 1
const MAX_LINE_BYTES := 1048576
const TOKEN_FILE := "user://osiris_ipc_token"
const MAX_POLL_TIME_MSEC := 5
const MAX_MESSAGES_PER_POLL := 10

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
var _run_pid: int = 0
var _run_log_path: String = ""
var _run_deadline_ms: int = 0
var _debug_errors: Array = []
var _debug_output: Array = []
var _debug_tree: Array = []
var _debug_metric_names: Array = []
var _debug_metrics: Array = []

func is_running() -> bool:
	return _editor.is_playing_scene()

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
	print("[OsirisBridge] metric_names recibidos: %d" % _debug_metric_names.size())

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
	print_rich("[OsirisBridge] escuchando en %s:%d" % [addr, port])

func stop() -> void:
	for p: StreamPeerTCP in _peers:
		p.disconnect_from_host()
	_peers.clear()
	_peer_buffers.clear()
	if _tcp:
		_tcp.stop()
		_tcp = null

func poll() -> void:
	if _busy:
		return
	_busy = true
	# Timeout de ejecución real (segundos), no frames (RF-09).
	if _run_deadline_ms > 0 and Time.get_ticks_msec() >= _run_deadline_ms:
		_run_deadline_ms = 0
		_editor.stop_playing_scene()
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
	_busy = false


func _accept() -> void:
	while _tcp and _tcp.is_connection_available():
		var p: StreamPeerTCP = _tcp.take_connection()
		if p:
			_peers.append(p)

func _process_peer(peer: StreamPeerTCP) -> int:
	var buf: String = _peer_buffers.get(peer, "")
	var avail: int = peer.get_available_bytes()
	if avail > 0:
		buf += peer.get_utf8_string(avail)

	var newline: int = buf.find("\n")
	if newline < 0:
		_peer_buffers[peer] = buf
		return 0

	var line: String = buf.substr(0, newline).strip_edges()
	buf = buf.substr(newline + 1)
	_peer_buffers[peer] = buf
	if line.length() > MAX_LINE_BYTES:
		return 0

	var test_json_conv: JSON = JSON.new()
	if test_json_conv.parse(line) != OK:
		return 0
	var msg: Dictionary = test_json_conv.get_data()
	if typeof(msg) != TYPE_DICTIONARY:
		return 0

	var response: Variant = _dispatch(msg)
	if response:
		_send(peer, response)
		return 1
	return 0

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
	match op:
		"ping":
			return _ok(id, {"pong": true})
		"get_version":
			return _get_version(id)
		"get_active_scene":
			return _get_active_scene(id)
		"list_nodes":
			return _list_nodes(id, msg.get("params", {}))
		"get_property":
			return _get_property(id, msg.get("params", {}))
		"list_resources":
			return _list_resources(id, msg.get("params", {}))
		"get_editor_errors":
			return _get_editor_errors(id)
		"create_node":
			return _create_node(id, msg.get("params", {}))
		"remove_node":
			return _remove_node(id, msg.get("params", {}))
		"set_property":
			return _set_property(id, msg.get("params", {}))
		"call_method":
			return _call_method(id, msg.get("params", {}))
		"save_scene":
			return _save_scene(id)
		"run_project":
			return _run_project(id, msg.get("params", {}))

		"stop_project":
			return _stop_project(id)
		"get_run_status":
			return _get_run_status(id)
		"get_run_output":
			return _get_run_output(id)
		"assert_property":
			return _assert_property(id, msg.get("params", {}))
		"validate_script":
			return _validate_script(id, msg.get("params", {}))
		"write_script":
			return _write_script(id, msg.get("params", {}))
		"describe_class":
			return _describe_class(id, msg.get("params", {}))
		"list_signals":
			return _list_signals(id, msg.get("params", {}))
		"connect_signal":
			return _connect_signal(id, msg.get("params", {}))
		"disconnect_signal":
			return _disconnect_signal(id, msg.get("params", {}))
		"asset_list":
			return _asset_list(id, msg.get("params", {}))
		"project_files":
			return _project_files(id, msg.get("params", {}) if typeof(msg.get("params")) == TYPE_DICTIONARY else {})
		"asset_reimport":
			return _asset_reimport(id, msg.get("params", {}))
		"resource_assign":
			return _resource_assign(id, msg.get("params", {}))
		"runtime_tree":
			return _runtime_tree(id)
		"runtime_metrics":
			return _runtime_metrics(id)
		_:
			return _error(id, "validation", "operación desconocida: %s" % op, false)

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
			args.append(a.get("name", ""))
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
	var script := GDScript.new()
	script.source_code = code
	var err: int = script.reload()
	if err != OK:
		return _error(id, "validation", "error de sintaxis en el script (código %d)" % err)
	return _ok(id, {"valid": true})


func _get_version(id: Variant) -> Dictionary:
	var vi: Dictionary = Engine.get_version_info()
	var info: Dictionary = {
		"godot": "%d.%d.%d" % [vi.major, vi.minor, vi.patch],
		"project": ProjectSettings.get_setting("application/config/name", ""),
	}
	return _ok(id, info)

func _get_active_scene(id: Variant) -> Dictionary:
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
			for i in range(mini(v.size(), 100)):
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

func _get_editor_errors(id: Variant) -> Dictionary:
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
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	var method: String = params.get("method", "")
	if method == "":
		return _error(id, "validation", "parámetro 'method' requerido")
	var raw_args: Array = params.get("args", [])
	var args: Array = []
	for a in raw_args:
		args.append(_json_to_variant(a))
	var result: Variant
	if args.size() == 0:
		result = node.call(method)
	else:
		result = node.callv(method, args)
	return _ok(id, {
		"path": params.get("path", "."),
		"method": method,
		"result": _variant_to_json(result),
	})

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

func _remove_node(id: Variant, params: Dictionary) -> Dictionary:
	var node: Node = _resolve_node(params)
	if not node:
		return _error(id, "godot", "nodo no encontrado")
	if node == _editor.get_edited_scene_root():
		return _error(id, "godot", "no se puede eliminar la raíz de la escena")
	var node_name: String = node.name
	var parent: Node = node.get_parent()
	node.queue_free()
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

func _save_scene(id: Variant) -> Dictionary:
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
	_run_pid = 0
	# Timeout real en segundos, revisado en poll() (no frames).
	var timeout_secs: int = int(params.get("timeout_secs", 0))
	_run_deadline_ms = Time.get_ticks_msec() + timeout_secs * 1000 if timeout_secs > 0 else 0
	return _ok(id, {"running": true, "scene": scene if scene != "" else "main", "pid": 0})


func _stop_project(id: Variant) -> Dictionary:
	_run_pid = 0
	_run_deadline_ms = 0
	_editor.stop_playing_scene()
	return _ok(id, {"running": false})

func _get_run_status(id: Variant) -> Dictionary:
	var running: bool = _editor.is_playing_scene()
	if not running:
		_run_deadline_ms = 0
	return _ok(id, {"running": running})

func _get_run_output(id: Variant) -> Dictionary:
	var lines: Array = _debug_output
	var tail: Array = lines.slice(maxi(lines.size() - 200, 0))
	return _ok(id, {"output": "\n".join(tail), "running": _editor.is_playing_scene()})

func _runtime_tree(id: Variant) -> Dictionary:
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

func _runtime_metrics(id: Variant) -> Dictionary:
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
