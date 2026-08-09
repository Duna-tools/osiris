@tool
class_name OsirisBridgeServer
extends RefCounted

const PROTOCOL_VERSION := 1
const MAX_LINE_BYTES := 1048576
const TOKEN_FILE := "user://osiris_ipc_token"
const MAX_POLL_TIME_MSEC := 5
const MAX_MESSAGES_PER_POLL := 10

var _tcp: TCPServer = null
var _peers: Array = []
var _peer_buffers: Dictionary = {}
var _busy: bool = false
var _token: String = ""
var _editor: EditorInterface = null
var _undo_redo: EditorUndoRedoManager = null
var _run_pid: int = 0
var _run_log_path: String = ""

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
		"describe_class":
			return _describe_class(id, msg.get("params", {}))
		_:
			return _error(id, "validation", "operación desconocida: %s" % op, false)

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
	return {"type": "response", "id": id, "ok": payload}

func _error(id: Variant, code: String, message: String, retryable: bool = false) -> Dictionary:
	return {"type": "response", "id": id if id != null else null, "error": {"code": code, "message": message, "retryable": retryable}}

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
	# Godot 4 no expone API pública para leer los errores del editor.
	return _ok(id, {"errors": [], "note": "Godot no expone errores del editor por API; el listado estará vacío hasta que la API exista"})

func _resolve_node(params: Dictionary) -> Node:
	var root: Node = _editor.get_edited_scene_root()
	var raw: String = params.get("path", ".")
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
	ok = ResourceSaver.save(packed, path)
	if ok != OK:
		return _error(id, "godot", "ResourceSaver.save devolvio %d" % ok)
	return _ok(id, {"saved": path})

func _validate_path(raw: String) -> String:
	if raw == "" or raw == ".":
		return "."
	if raw.contains("..") or raw.contains("//") or raw.begins_with("/"):
		return ""
	return raw

func _run_project(id: Variant, params: Dictionary) -> Dictionary:
	var scene: String = params.get("scene", "")
	if scene != "":
		var valid_scene: String = _validate_path(scene)
		if valid_scene == "":
			return _error(id, "validation", "ruta de escena inválida: %s" % scene)
		_editor.play_custom_scene(valid_scene)
	else:
		_editor.play_main_scene()
	# Captura de salida del proyecto ejecutado (RF-028/UC-030).
	_run_log_path = ProjectSettings.globalize_path("user://osiris_run_output.log")
	var project_dir: String = ProjectSettings.globalize_path("res://")
	var godot_bin: String = OS.get_executable_path()
	var args: PackedStringArray = ["--path", project_dir, "--quit-after", "600"]
	if scene != "":
		args.append(scene)
	args.append("--log-file")
	args.append(_run_log_path)
	var pid: int = OS.create_process(godot_bin, args, false)
	if pid == 0:
		return _error(id, "godot", "no se pudo lanzar el proceso Godot")
	_run_pid = pid
	return _ok(id, {"running": true, "scene": scene if scene != "" else "main", "pid": pid})


func _stop_project(id: Variant) -> Dictionary:
	if _run_pid > 0:
		OS.kill(_run_pid)
		_run_pid = 0
	_editor.stop_playing_scene()
	return _ok(id, {"running": false})

func _get_run_status(id: Variant) -> Dictionary:
	return _ok(id, {"running": _run_pid > 0 or _editor.is_playing_scene()})

func _get_run_output(id: Variant) -> Dictionary:
	if _run_log_path == "":
		return _ok(id, {"output": "", "running": _get_run_status(id)["running"]})
	var f: FileAccess = FileAccess.open(_run_log_path, FileAccess.READ)
	var lines: PackedStringArray = []
	if f:
		while not f.eof_reached():
			lines.append(f.get_line())
		f.close()
	var tail: PackedStringArray = lines.slice(maxi(lines.size() - 200, 0))
	return _ok(id, {"output": "\n".join(tail), "running": _get_run_status(id)["running"]})

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
