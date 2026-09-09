#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_PROFILE=debug
CARGO_ARGS=()
case "${1:-}" in
  "") ;;
  --release) BUILD_PROFILE=release; CARGO_ARGS=(--release) ;;
  *) echo "Usage: $0 [--release]" >&2; exit 2 ;;
esac
cd "$REPO_ROOT"
cargo build "${CARGO_ARGS[@]}" -p toge-slint -p toged
DEV_PROFILE="${TOGE_DEV_PROFILE:-slint}"
case "$DEV_PROFILE" in
  ""|*[!A-Za-z0-9._-]*) echo "Invalid TOGE_DEV_PROFILE" >&2; exit 2 ;;
esac
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
export XDG_CONFIG_HOME="$DEV_CONFIG_ROOT/$DEV_PROFILE"
export XDG_STATE_HOME="$DEV_RUNTIME_DIR/state"
export TOGE_SOCKET="$DEV_RUNTIME_DIR/toged.sock"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"
echo "Slint development configuration: $XDG_CONFIG_HOME/toge/config.toml"
"$REPO_ROOT/target/$BUILD_PROFILE/toged" --socket "$TOGE_SOCKET" &
PID_DAEMON=$!
# Wait for socket creation so the GUI does not launch a second daemon.
for ((attempt=0; attempt<100; attempt++)); do
  [ -S "$TOGE_SOCKET" ] && break
  kill -0 "$PID_DAEMON" 2>/dev/null || { echo "Development daemon exited" >&2; exit 1; }
  sleep 0.05
done
[ -S "$TOGE_SOCKET" ] || { echo "Daemon socket did not appear" >&2; exit 1; }
"$REPO_ROOT/target/$BUILD_PROFILE/toge-slint"
