#!/usr/bin/env bash
# Launch VSCode (Extension Development Host) under Xvfb and take the SPEC screenshots.
# The shots are taken over CDP with screenshots/shot.mjs.
#
# Usage: screenshots/run.sh [outdir]
#   outdir defaults to screenshots/dist (gitignored).
#   Produces 01-highlighting.png, 02-fold-comments.png,
#   03-fold-headings.png
#   as specified by SPEC.md §スクショ, opening screenshots/sample.todo.
#
# Notes:
# - WSLg sockets are unreachable from the agent sandbox; Xvfb provides the display.
# - Every run owns its resources, so concurrent runs (or leaked processes from
#   an earlier run) cannot collide: the Xvfb display is picked among free
#   displays, Chromium picks a free CDP port (--remote-debugging-port=0; the
#   actual port is read from DevToolsActivePort), and HOME, user-data-dir,
#   extensions-dir and the logs live in a per-run mktemp -d directory. The run
#   directory is removed on success and kept behind on failure (its path is
#   printed to stderr) for debugging.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$REPO/screenshots/dist}"
EXTDIR="$REPO/vscode-todo"
SHOT="$(dirname "$0")/shot.mjs"
SAMPLE="$(dirname "$0")/sample.todo"

RUN_DIR="$(mktemp -d /tmp/vscode-shot.XXXXXX)"
DATA_DIR="$RUN_DIR/data"
HOME_DIR="$RUN_DIR/home"
XVFB_LOG="$RUN_DIR/xvfb.log"
VSCODE_LOG="$RUN_DIR/vscode-electron.log"

VSCODE_PID=""
XVFB_PID=""

cleanup() {
  local status=$?
  # TERM first (Xvfb removes its lock file on graceful exit), then KILL after
  # a short grace period so nothing writes into the run dir while it is removed.
  kill "$VSCODE_PID" "$XVFB_PID" 2>/dev/null || true
  sleep 1
  kill -9 "$VSCODE_PID" "$XVFB_PID" 2>/dev/null || true
  if [ "$status" -eq 0 ]; then
    rm -rf "$RUN_DIR"
  else
    echo "run dir kept for debugging: $RUN_DIR" >&2
  fi
}
trap cleanup EXIT

mkdir -p "$DATA_DIR/User" "$HOME_DIR/.vscode"
echo '{}' > "$HOME_DIR/.vscode/argv.json"
cat > "$DATA_DIR/User/settings.json" <<'EOF'
{
  "security.workspace.trust.enabled": false,
  "workbench.startupEditor": "none",
  "update.mode": "none",
  "extensions.ignoreRecommendations": true,
  "chat.disableAIFeatures": true,
  "todo-language.repeatTask.autoRepeat": false,
  "window.newWindowDimensions": "maximized",
  "editor.minimap.enabled": false,
  "editor.fontSize": 20,
  "json.validate.enable": false,
  "timeline.enabled": false,
  "breadcrumbs.enabled": false
}
EOF

# Pick a free display: skip displays with a lock file; if Xvfb still fails to
# start (race with another starter), fall through to the next candidate.
DISPLAY_NUM=""
for n in $(seq 99 118); do
  if [ -e "/tmp/.X$n-lock" ]; then continue; fi
  Xvfb ":$n" -screen 0 1600x1400x24 -nolisten tcp > "$XVFB_LOG" 2>&1 &
  XVFB_PID=$!
  sleep 1
  if kill -0 "$XVFB_PID" 2>/dev/null; then DISPLAY_NUM=$n; break; fi
  wait "$XVFB_PID" 2>/dev/null || true
  XVFB_PID=""
done
if [ -z "$DISPLAY_NUM" ]; then
  echo "no free display for xvfb; see $XVFB_LOG" >&2
  exit 1
fi

# env -u VSCODE_IPC_HOOK_CLI: otherwise bin/code forwards to the remote-cli of the
# running Windows VSCode and the launch flags are silently ignored.
# --remote-debugging-port=0: Chromium picks a free port and records it in
# DevToolsActivePort (first line) once the CDP server is listening.
PORT_FILE="$DATA_DIR/DevToolsActivePort"
env -u VSCODE_IPC_HOOK_CLI \
  HOME="$HOME_DIR" DISPLAY=":$DISPLAY_NUM" DBUS_SESSION_BUS_ADDRESS="disabled:" \
  ~/apps/vscode/code --no-sandbox --disable-gpu \
  --user-data-dir="$DATA_DIR" --extensions-dir="$RUN_DIR/ext" \
  --remote-debugging-port=0 --remote-allow-origins='*' \
  --extensionDevelopmentPath="$EXTDIR" \
  "$REPO" "$SAMPLE" > "$VSCODE_LOG" 2>&1 &
VSCODE_PID=$!

UP=0
DIED=0
CDP_PORT=""
for _ in $(seq 1 40); do
  if [ -s "$PORT_FILE" ]; then CDP_PORT="$(head -n1 "$PORT_FILE")"; UP=1; break; fi
  if ! kill -0 "$VSCODE_PID" 2>/dev/null; then DIED=1; echo "vscode died; see $VSCODE_LOG" >&2; break; fi
  sleep 1
done
if [ "$UP" = 0 ] && [ "$DIED" = 0 ]; then
  echo "timeout waiting for vscode CDP; see $VSCODE_LOG" >&2
fi

SHOT_STATUS=1
if [ "$UP" = 1 ]; then
  for _ in $(seq 1 30); do
    if curl -s -m 2 "http://127.0.0.1:$CDP_PORT/json" 2>/dev/null | grep -q 'vscode-file://'; then break; fi
    sleep 1
  done
  sleep 8   # extension activation + LSP startup + first semantic tokens
  if node "$SHOT" "$OUT" --port "$CDP_PORT" --wait 3000; then
    SHOT_STATUS=0
  else
    SHOT_STATUS=$?
  fi
fi

if [ "$UP" = 1 ] && [ "$SHOT_STATUS" = 0 ]; then
  exit 0
fi
exit 1
