#!/usr/bin/env bash
# Wrapper de integración con clientes MCP para el servidor Osiris.
#
# Detecta el proyecto Godot desde el cwd (busca project.godot hacia arriba),
# lee el token del bridge en .godot/osiris_ipc_token y lanza osiris-server con
# el perfil admin.
#
# Si hay un proyecto Godot pero el bridge no está activo, lanza Godot
# automáticamente (el editor se abre y el MCP queda conectado sin intervención).
#
# Requisitos: osiris-server en PATH, o OSIRIS_SERVER apuntando al binario.
# El binario godot debe estar en PATH.

set -euo pipefail

BIN="${OSIRIS_SERVER:-/home/luke/Documentos/Projects/osiris/target/debug/osiris-server}"

# Buscar project.godot desde el cwd hacia arriba.
PROJECT_DIR=""
DIR="$(pwd)"
while [ "$DIR" != "/" ]; do
  if [ -f "$DIR/project.godot" ]; then
    PROJECT_DIR="$DIR"
    break
  fi
  DIR="$(dirname "$DIR")"
done

TOKEN_FILE="${PROJECT_DIR:+$PROJECT_DIR/.godot/osiris_ipc_token}"

if [ -n "$PROJECT_DIR" ]; then
  # El token puede existir pero el editor puede estar cerrado (stale).
  # Verificar que el puente realmente escucha.
  if ! ss -tln 2>/dev/null | grep -q ":9876"; then
    echo "[osiris-mcp] lanzando Godot en $PROJECT_DIR..." >&2
    setsid godot -e --path "$PROJECT_DIR" > /dev/null 2>&1 < /dev/null &
    for i in $(seq 1 20); do
      sleep 1
      ss -tln 2>/dev/null | grep -q ":9876" && break
    done
    echo "[osiris-mcp] bridge listo" >&2
  fi
  if [ -f "$TOKEN_FILE" ]; then
    exec "$BIN" --ipc-addr 127.0.0.1:9876 --ipc-token-file "$TOKEN_FILE" --capability-profile admin
  fi
fi

exec "$BIN" --capability-profile admin
