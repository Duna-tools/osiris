@tool
class_name OsirisBridgeServer
extends RefCounted

const PROTOCOL_VERSION := 1
const MAX_LINE_BYTES := 1048576
const TOKEN_FILE := "user://osiris_ipc_token"

var _tcp: TCPServer = null
var _peers: Array = []
var _peer_buffers: Dictionary = {}
var _busy: bool = false
var _token: String = ""
var _editor: EditorInterface = null

func start(addr: String, port: int, editor: EditorInterface) -> void:
	_tcp = TCPServer.new()
	if _tcp.listen(port, addr) != OK:
		push_error("[OsirisBridge] no se puede escuchar en %s:%d" % [addr, port])
		_tcp = null
		return
	_token = _generate_token()
	_write_token()
	_editor = editor
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
	var to_remove: Array = []
	for p: StreamPeerTCP in _peers:
		if p.get_status() != StreamPeerTCP.STATUS_CONNECTED:
			to_remove.append(p)
		else:
			_process_peer(p)
	for p in to_remove:
		_peers.erase(p)
		_peer_buffers.erase(p)
	_busy = false

func _accept() -> void:
	while _tcp and _tcp.is_connection_available():
		var p: StreamPeerTCP = _tcp.take_connection()
		if p:
			_peers.append(p)

func _process_peer(peer: StreamPeerTCP) -> void:
	var buf: String = _peer_buffers.get(peer, "")
	var avail: int = peer.get_available_bytes()
	if avail > 0:
		buf += peer.get_utf8_string(avail)

	var newline: int = buf.find("\n")
	if newline < 0:
		_peer_buffers[peer] = buf
		return

	var line: String = buf.substr(0, newline).strip_edges()
	buf = buf.substr(newline + 1)
	_peer_buffers[peer] = buf
	if line.length() > MAX_LINE_BYTES:
		return

	var test_json_conv: JSON = JSON.new()
	if test_json_conv.parse(line) != OK:
		return
	var msg: Dictionary = test_json_conv.get_data()
	if typeof(msg) != TYPE_DICTIONARY:
		return

	var response: Variant = _dispatch(msg)
	if response:
		_send(peer, response)

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
			return _run_project(id)
		"stop_project":
			return _stop_project(id)
		"get_run_status":
			return _get_run_status(id)
		"assert_property":
			return _assert_property(id, msg.get("params", {}))
		_:
			return _error(id, "validation", "operación desconocida: %s" % op, false)

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
	node.set(prop, _json_to_variant(value))
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

func _run_project(id: Variant) -> Dictionary:
	_editor.play_main_scene()
	return _ok(id, {"running": true})

func _stop_project(id: Variant) -> Dictionary:
	_editor.stop_playing_scene()
	return _ok(id, {"running": false})

func _get_run_status(id: Variant) -> Dictionary:
	return _ok(id, {"running": _editor.is_playing_scene()})

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
