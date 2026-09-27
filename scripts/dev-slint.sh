#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_ARGS=()
case "${1:-}" in
  "") ;;
  --release) CARGO_ARGS=(--release) ;;
  *) echo "Usage: $0 [--release]" >&2; exit 2 ;;
esac
cd "$REPO_ROOT"
DEV_PROFILE="${TOGE_DEV_PROFILE:-slint}"
case "$DEV_PROFILE" in
  ""|*[!A-Za-z0-9._-]*) echo "Invalid TOGE_DEV_PROFILE" >&2; exit 2 ;;
esac
# Cargo may use CARGO_TARGET_DIR, a configured target directory, or a target
# triple. Run the executables produced by this build, never a guessed old path.
BUILD_OUTPUT="$(mktemp /tmp/toge-slint-build.XXXXXX)"
trap 'rm -f "$BUILD_OUTPUT"' EXIT
BUILD_STATUS=0
cargo build "${CARGO_ARGS[@]}" -p toge-slint -p toged --message-format=json-render-diagnostics >"$BUILD_OUTPUT" || BUILD_STATUS=$?
BUILT_EXECUTABLES="$(python3 - "$BUILD_OUTPUT" "$BUILD_STATUS" <<'PYTHON'
import json
import os
import sys

executables = {}
with open(sys.argv[1]) as messages:
    for line in messages:
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            rendered = message.get("message", {}).get("rendered")
            if rendered:
                print(rendered, file=sys.stderr, end="")
        if message.get("reason") == "compiler-artifact":
            target = message.get("target", {})
            executable = message.get("executable")
            if "bin" in target.get("kind", []) and executable:
                executables[target["name"]] = executable
if int(sys.argv[2]):
    sys.exit(int(sys.argv[2]))
for name in ("toge-slint", "toged"):
    executable = executables.get(name)
    if not executable or not os.path.isfile(executable) or not os.access(executable, os.X_OK):
        sys.exit(f"Cargo did not report a runnable {name} executable; refusing to launch an old build.")
for name in ("toge-slint", "toged"):
    print(executables[name])
PYTHON
)"
mapfile -t BUILT_PATHS <<<"$BUILT_EXECUTABLES"
SLINT_BIN="${BUILT_PATHS[0]}"
DAEMON_BIN="${BUILT_PATHS[1]}"
rm -f "$BUILD_OUTPUT"
trap - EXIT
echo "Slint executable: $SLINT_BIN"
echo "Daemon executable: $DAEMON_BIN"
# Ask through the native Slint UI if this build lost its file capabilities.
"$SLINT_BIN" --request-watcher-access "$DAEMON_BIN"
bash "$REPO_ROOT/scripts/check-toged-capabilities.sh" "$DAEMON_BIN"
DEV_RUNTIME_DIR="$(mktemp -d /tmp/toge-slint-dev.XXXXXX)"
PID_DAEMON=""
cleanup() {
  if [ -n "$PID_DAEMON" ]; then
    kill "$PID_DAEMON" 2>/dev/null || true
    wait "$PID_DAEMON" 2>/dev/null || true
  fi
  rm -rf "$DEV_RUNTIME_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
DEV_CONFIG_ROOT="${TOGE_DEV_CONFIG_ROOT:-${XDG_CONFIG_HOME:-$HOME/.config}/toge-dev}"
DEV_STATE_ROOT="${TOGE_DEV_STATE_ROOT:-${XDG_STATE_HOME:-$HOME/.local/state}/toge-dev}"
export XDG_CONFIG_HOME="$DEV_CONFIG_ROOT/$DEV_PROFILE"
export XDG_STATE_HOME="$DEV_STATE_ROOT/$DEV_PROFILE"
export TOGE_SOCKET="$DEV_RUNTIME_DIR/toged.sock"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
chmod 700 "$XDG_STATE_HOME"
# Separate launches use temporary sockets but share this profile's saved index.
# Prevent concurrent daemons from writing that index at the same time.
exec 9>"$XDG_STATE_HOME/launcher.lock"
if ! flock -n 9; then
  echo "Slint development profile '$DEV_PROFILE' is already running; use another TOGE_DEV_PROFILE." >&2
  exit 1
fi
echo "Slint development index: $XDG_STATE_HOME/toge/index.bin"
echo "Slint development configuration: $XDG_CONFIG_HOME/toge/config.toml"
"$DAEMON_BIN" --socket "$TOGE_SOCKET" 9>&- &
PID_DAEMON=$!
# Wait for socket creation so the GUI does not launch a second daemon.
for ((attempt=0; attempt<100; attempt++)); do
  [ -S "$TOGE_SOCKET" ] && break
  kill -0 "$PID_DAEMON" 2>/dev/null || { echo "Development daemon exited" >&2; exit 1; }
  sleep 0.05
done
[ -S "$TOGE_SOCKET" ] || { echo "Daemon socket did not appear" >&2; exit 1; }
"$SLINT_BIN" 9>&-
