@tool
class_name OsirisDebuggerController
extends EditorDebuggerPlugin

## Controla el debugger del editor: pide el árbol remoto y métricas del juego.
## El plugin EditorDebuggerPlugin._capture NO recibe error/output (handlers
## nativos), pero sus sesiones SÍ exponen send_message() para pedir datos.
## Ver vault 18_errores_godot.md §7.

var _session: Object = null

func _setup_session(session_id: int) -> void:
	_session = get_session(session_id)

func request_tree() -> void:
	if _session:
		_session.send_message("scene:request_scene_tree", [])

func request_inspect(ids: Array) -> void:
	if _session:
		_session.send_message("scene:inspect_objects", [ids, false])

var _screenshot_id: int = 0

func request_screenshot() -> int:
	_screenshot_id += 1
	if _session:
		_session.send_message("scene:rq_screenshot", [_screenshot_id])
	return _screenshot_id

func enable_metrics() -> void:
	if _session:
		_session.toggle_profiler("performance", true, [])
