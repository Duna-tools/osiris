#!/usr/bin/env bash
# Instala Osiris como MCP server en todos los CLIs de LLM detectados
# (opencode, codex, agy, claude). Idempotente: no duplica entradas existentes.
#
# El wrapper osiris-mcp.sh detecta el proyecto Godot desde el cwd y conecta al
# bridge. El indicador "Osiris activo" aparece en la barra inferior del CLI:
# opencode muestra "⊙ N MCP" (el contador sube con Osiris conectado).
#
# Uso: osiris-install.sh [--remove]
#   (sin args)  registra Osiris en todos los CLIs detectados
#   --remove    quita la entrada de Osiris de todos los CLIs

set -euo pipefail

OSIRIS_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WRAPPER="$OSIRIS_ROOT/scripts/osiris-mcp.sh"
MODE="${1:-install}"
MODE="${MODE#--}"

echo "Osiris root: $OSIRIS_ROOT"
echo "Wrapper:     $WRAPPER"

# --- opencode (~/.config/opencode/opencode.json) ---------------------------
install_opencode() {
  local cfg="$HOME/.config/opencode/opencode.json"
  [ -f "$cfg" ] || return 0
  echo "[opencode] $cfg"
  python3 - "$cfg" "$WRAPPER" "$MODE" <<'EOF'
import json, sys
cfg, wrapper, mode = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(cfg))
mcp = d.setdefault("mcp", {})
entry = {"type": "local", "command": [wrapper], "enabled": True}
if mode == "remove":
    mcp.pop("osiris", None)
    print("  -> osiris quitado")
else:
    if "osiris" in mcp:
        print("  -> osiris ya estaba (sin cambios)")
    else:
        mcp["osiris"] = entry
        print("  -> osiris registrado")
json.dump(d, open(cfg, "w"), indent=2, ensure_ascii=False)
open(cfg, "a").write("\n")
EOF
}

# --- codex (~/.codex/config.toml) ------------------------------------------
# TOML: añadir el bloque solo si no existe; nunca reescribir el archivo entero
# (evita romper secciones existentes). El bloque se añade al final.
install_codex() {
  local cfg="$HOME/.codex/config.toml"
  [ -f "$cfg" ] || return 0
  echo "[codex] $cfg"
  local block="[mcp_servers.osiris]
command = \"$WRAPPER\"
args = []
"
  if [ "$MODE" = "remove" ]; then
    python3 - "$cfg" <<'EOF'
import sys
cfg = sys.argv[1]
lines = open(cfg).read().splitlines()
out, skipping = [], False
for ln in lines:
    if ln.strip() == "[mcp_servers.osiris]":
        skipping = True
        continue
    if skipping:
        if ln.strip().startswith("["):
            skipping = False
        elif ln.strip():
            continue
    if not skipping:
        out.append(ln)
open(cfg, "w").write("\n".join(out).rstrip() + "\n")
print("  -> osiris quitado")
EOF
  elif ! grep -qF "[mcp_servers.osiris]" "$cfg"; then
    printf "\n%s" "$block" >> "$cfg"
    echo "  -> osiris registrado"
  else
    echo "  -> osiris ya estaba (sin cambios)"
  fi
}

# --- agy / Google Antigravity (~/.gemini/config/mcp_config.json) -----------
install_agy() {
  local cfg="$HOME/.gemini/config/mcp_config.json"
  [ -f "$cfg" ] || return 0
  echo "[agy] $cfg"
  python3 - "$cfg" "$WRAPPER" "$MODE" <<'EOF'
import json, sys
cfg, wrapper, mode = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(cfg))
mcp = d.setdefault("mcpServers", {})
if mode == "remove":
    mcp.pop("osiris", None)
    print("  -> osiris quitado")
else:
    if "osiris" in mcp:
        print("  -> osiris ya estaba (sin cambios)")
    else:
        mcp["osiris"] = {"command": wrapper}
        print("  -> osiris registrado")
json.dump(d, open(cfg, "w"), indent=2, ensure_ascii=False)
open(cfg, "a").write("\n")
EOF
}

# --- claude (~/.claude.json) ----------------------------------------------
install_claude() {
  local cfg="$HOME/.claude.json"
  [ -f "$cfg" ] || return 0
  echo "[claude] $cfg"
  python3 - "$cfg" "$WRAPPER" "$MODE" <<'EOF'
import json, sys
cfg, wrapper, mode = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(cfg))
mcp = d.setdefault("mcpServers", {})
if mode == "remove":
    mcp.pop("osiris", None)
    print("  -> osiris quitado")
else:
    if "osiris" in mcp:
        print("  -> osiris ya estaba (sin cambios)")
    else:
        mcp["osiris"] = {"command": wrapper}
        print("  -> osiris registrado")
json.dump(d, open(cfg, "w"), indent=2, ensure_ascii=False)
EOF
}

install_opencode
install_codex
install_agy
install_claude

echo ""
echo "Listo. Reinicia el CLI para que cargue la entrada MCP."
echo "Indicador de Osiris activo: en opencode se ve como '⊙ N MCP' en la barra inferior."
