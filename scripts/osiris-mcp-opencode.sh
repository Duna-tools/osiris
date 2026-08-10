#!/usr/bin/env bash
# Wrapper de opencode para el MCP server de Osiris.
#
# Detecta el proyecto Godot desde el cwd (busca project.godot hacia arriba),
# lee el token del bridge en .godot/osiris_ipc_token y lanza osiris-server con
# el perfil admin. Si no hay proyecto Godot o el bridge no está activo, arranca
# igual: las tools locales (diagnose, skills, capabilities, run.history) siguen
# disponibles y las de Godot devuelven error claro.
#
# Requisitos: osiris-server en PATH, o OSIRIS_SERVER apuntando al binario.

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

if [ -n "$PROJECT_DIR" ] && [ -f "$TOKEN_FILE" ]; then
  exec "$BIN" --ipc-addr 127.0.0.1:9876 --ipc-token-file "$TOKEN_FILE" --capability-profile admin
else
  exec "$BIN" --capability-profile admin
fi
