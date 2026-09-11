#!/usr/bin/env bash
# Launch VSCode (Extension Development Host) under Xvfb and take the SPEC screenshots.
# The shots are taken over CDP with screenshots/shot.mjs.
#
# Usage: screenshots/run.sh [outdir]
#   outdir defaults to screenshots/dist (gitignored).
#   Produces 01-complete.png, 02-highlighting.png, 03-fold-comments.png,
#   04-fold-headings.png
#   as specified by SPEC.md §スクショ, opening screenshots/sample.todo.
#
# Notes:
# - WSLg sockets are unreachable from the agent sandbox; Xvfb provides the display.
# - HOME points at /tmp so ~/.vscode/argv.json is writable (silences a startup warning).
# - user-data-dir/extensions-dir live in /tmp: every run starts from a clean, reproducible state.
set -u

REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$REPO/screenshots/dist}"
EXTDIR="$REPO/vscode-todo"
SHOT="$(dirname "$0")/shot.mjs"
SAMPLE="$(dirname "$0")/sample.todo"

rm -rf /tmp/vscode-shot-data /tmp/vscode-shot-ext /tmp/vscode-home
mkdir -p /tmp/vscode-shot-data/User /tmp/vscode-home/.vscode
echo '{}' > /tmp/vscode-home/.vscode/argv.json
cat > /tmp/vscode-shot-data/User/settings.json <<'EOF'
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

Xvfb :99 -screen 0 1600x1400x24 -nolisten tcp > /tmp/xvfb.log 2>&1 &
XVFB_PID=$!
sleep 1

# env -u VSCODE_IPC_HOOK_CLI: otherwise bin/code forwards to the remote-cli of the
# running Windows VSCode and the launch flags are silently ignored.
HOME=/tmp/vscode-home DISPLAY=:99 DBUS_SESSION_BUS_ADDRESS="disabled:" \
  ~/apps/vscode/code --no-sandbox --disable-gpu \
  --user-data-dir=/tmp/vscode-shot-data --extensions-dir=/tmp/vscode-shot-ext \
  --remote-debugging-port=9222 --remote-allow-origins='*' \
  --extensionDevelopmentPath="$EXTDIR" \
  "$REPO" "$SAMPLE" > /tmp/vscode-electron.log 2>&1 &
VSCODE_PID=$!

UP=0
DIED=0
for _ in $(seq 1 40); do
  if curl -s -m 2 http://127.0.0.1:9222/json/version 2>/dev/null | grep -q webSocketDebuggerUrl; then UP=1; break; fi
  if ! kill -0 "$VSCODE_PID" 2>/dev/null; then DIED=1; echo "vscode died; see /tmp/vscode-electron.log" >&2; break; fi
  sleep 1
done
if [ "$UP" = 0 ] && [ "$DIED" = 0 ]; then
  echo "timeout waiting for vscode CDP; see /tmp/vscode-electron.log" >&2
fi

SHOT_STATUS=1
if [ "$UP" = 1 ]; then
  for _ in $(seq 1 30); do
    if curl -s -m 2 http://127.0.0.1:9222/json 2>/dev/null | grep -q 'vscode-file://'; then break; fi
    sleep 1
  done
  sleep 8   # extension activation + LSP startup + first semantic tokens
  node "$SHOT" "$OUT" --wait 3000
  SHOT_STATUS=$?
fi

kill "$VSCODE_PID" "$XVFB_PID" 2>/dev/null
if [ "$UP" = 1 ] && [ "$SHOT_STATUS" = 0 ]; then
  exit 0
fi
exit 1
