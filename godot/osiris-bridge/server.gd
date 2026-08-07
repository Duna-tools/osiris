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

func start(addr: String, port: int) -> void:
	_tcp = TCPServer.new()
	if _tcp.listen(port, addr) != OK:
		push_error("[OsirisBridge] no se puede escuchar en %s:%d" % [addr, port])
		_tcp = null
		return
	_token = _generate_token()
	_write_token()
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
	for p: StreamPeerTCP in _peers:
		if p.get_status() != StreamPeerTCP.STATUS_CONNECTED:
			_peers.erase(p)
			_peer_buffers.erase(p)
		else:
			_process_peer(p)
	_busy = false

func _accept() -> void:
	while _tcp and _tcp.is_connection_available():
		var p: StreamPeerTCP = _tcp.take_connection()
		if p:
			_peers.append(p)

func _process_peer(peer: StreamPeerTCP) -> void:
	var buf: String = _peer_buffers.get(peer, "")
	var chunk := peer.get_utf8_string(peer.get_available_bytes())
	if chunk:
		buf += chunk

	var newline := buf.find("\n")
	if newline < 0:
		_peer_buffers[peer] = buf
		return

	var line := buf.substr(0, newline).strip_edges()
	buf = buf.substr(newline + 1)
	_peer_buffers[peer] = buf
	if line.length() > MAX_LINE_BYTES:
		return

	var test_json_conv := JSON.new()
	if test_json_conv.parse(line) != OK:
		return
	var msg: Dictionary = test_json_conv.get_data()
	if typeof(msg) != TYPE_DICTIONARY:
		return

	var response := _dispatch(msg)
	if response:
		_send(peer, response)

func _send(peer: StreamPeerTCP, data: Dictionary) -> void:
	var txt := JSON.stringify(data, "", false)
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
	if tok != _token:
		return _hello_err("authentication", "token inválido")
	var vi: Dictionary = Engine.get_version_info()
	var ver := "%d.%d.%d" % [vi.major, vi.minor, vi.patch]
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
		_:
			return _error(id, "validation", "operación desconocida: %s" % op, false)

func _get_version(id: Variant) -> Dictionary:
	var vi: Dictionary = Engine.get_version_info()
	var info := {
		"godot": "%d.%d.%d" % [vi.major, vi.minor, vi.patch],
		"project": ProjectSettings.get_setting("application/config/name", ""),
	}
	return _ok(id, info)

func _ok(id: Variant, payload: Variant) -> Dictionary:
	return {"type": "response", "id": id, "ok": payload}

func _error(id: Variant, code: String, message: String, retryable: bool = false) -> Dictionary:
	return {"type": "response", "id": id if id != null else null, "error": {"code": code, "message": message, "retryable": retryable}}

func _generate_token() -> String:
	var bytes := PackedByteArray()
	bytes.resize(32)
	for i in 32:
		bytes[i] = randi() % 256
	return bytes.hex_encode()

func _write_token() -> void:
	var f := FileAccess.open(TOKEN_FILE, FileAccess.WRITE)
	if f:
		f.store_string(_token)
		f.close()
