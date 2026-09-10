#!/usr/bin/env bash
# Launch VSCode (Extension Development Host) under Xvfb and screenshot the workbench.
# The shot is taken over CDP with scripts/screenshot/shot.mjs.
#
# Usage: scripts/screenshot/run.sh [out.png] [file-to-open]
#   out.png defaults to screenshots/<timestamp>-vscode.png (gitignored).
#
# Notes:
# - WSLg sockets are unreachable from the agent sandbox; Xvfb provides the display.
# - HOME points at /tmp so ~/.vscode/argv.json is writable (silences a startup warning).
# - user-data-dir/extensions-dir live in /tmp: every run starts from a clean, reproducible state.
set -u

REPO="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$REPO/screenshots/$(date +%Y%m%d-%H%M%S)-vscode.png}"
OPEN_FILE="${2:-}"
EXTDIR="$REPO/vscode-todo"
SHOT="$(dirname "$0")/shot.mjs"

rm -rf /tmp/vscode-shot-data /tmp/vscode-shot-ext /tmp/vscode-home
mkdir -p /tmp/vscode-shot-data/User /tmp/vscode-home/.vscode
echo '{}' > /tmp/vscode-home/.vscode/argv.json
cat > /tmp/vscode-shot-data/User/settings.json <<'EOF'
{
  "security.workspace.trust.enabled": false,
  "workbench.startupEditor": "none",
  "update.mode": "none",
  "extensions.ignoreRecommendations": true,
  "chat.disableAIFeatures": true
}
EOF

Xvfb :99 -screen 0 1600x1000x24 -nolisten tcp > /tmp/xvfb.log 2>&1 &
XVFB_PID=$!
sleep 1

# env -u VSCODE_IPC_HOOK_CLI: otherwise bin/code forwards to the remote-cli of the
# running Windows VSCode and the launch flags are silently ignored.
HOME=/tmp/vscode-home DISPLAY=:99 DBUS_SESSION_BUS_ADDRESS="disabled:" \
  ~/apps/vscode/code --no-sandbox --disable-gpu \
  --user-data-dir=/tmp/vscode-shot-data --extensions-dir=/tmp/vscode-shot-ext \
  --remote-debugging-port=9222 --remote-allow-origins='*' \
  --extensionDevelopmentPath="$EXTDIR" \
  "$REPO" $OPEN_FILE > /tmp/vscode-electron.log 2>&1 &
VSCODE_PID=$!

UP=0
for _ in $(seq 1 40); do
  if curl -s -m 2 http://127.0.0.1:9222/json/version 2>/dev/null | grep -q webSocketDebuggerUrl; then UP=1; break; fi
  if ! kill -0 "$VSCODE_PID" 2>/dev/null; then echo "vscode died; see /tmp/vscode-electron.log" >&2; break; fi
  sleep 1
done

if [ "$UP" = 1 ]; then
  for _ in $(seq 1 30); do
    if curl -s -m 2 http://127.0.0.1:9222/json 2>/dev/null | grep -q 'vscode-file://'; then break; fi
    sleep 1
  done
  sleep 8   # extension activation + LSP startup + first semantic tokens
  node "$SHOT" "$OUT" --wait 3000
fi

kill "$VSCODE_PID" "$XVFB_PID" 2>/dev/null
exit 0
