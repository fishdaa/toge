#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$SCRIPT_DIR/toge-gui"
PID_VITE=""
PID_DAEMON=""
DEV_RUNTIME_DIR=""
BUILD_PROFILE="debug"
CARGO_PROFILE_ARGS=()

case "${1:-}" in
  "") ;;
  --release)
    BUILD_PROFILE="release"
    CARGO_PROFILE_ARGS=(--release)
    ;;
  *)
    echo "Usage: $0 [--release]" >&2
    exit 2
    ;;
esac

DAEMON_BIN="$SCRIPT_DIR/target/$BUILD_PROFILE/toged"

cleanup() {
  if [ -n "$PID_DAEMON" ] && kill -0 "$PID_DAEMON" 2>/dev/null; then
    kill "$PID_DAEMON" 2>/dev/null || true
    wait "$PID_DAEMON" 2>/dev/null || true
  fi
  if [ -n "$PID_VITE" ] && kill -0 "$PID_VITE" 2>/dev/null; then
    kill "$PID_VITE" 2>/dev/null || true
    wait "$PID_VITE" 2>/dev/null || true
  fi
  if [ -n "$DEV_RUNTIME_DIR" ] && [ -d "$DEV_RUNTIME_DIR" ]; then
    rm -rf "$DEV_RUNTIME_DIR"
  fi
}
trap cleanup EXIT INT TERM

echo "Building daemon for this dev session ($BUILD_PROFILE)..."
cd "$SCRIPT_DIR"
cargo build "${CARGO_PROFILE_ARGS[@]}" -p toged
sudo setcap cap_sys_admin,cap_dac_read_search+ep "$DAEMON_BIN"

DEV_RUNTIME_DIR="$(mktemp -d /tmp/toge-gui-dev.XXXXXX)"
DEV_PROFILE="${TOGE_DEV_PROFILE:-default}"
case "$DEV_PROFILE" in
  ""|*[!A-Za-z0-9._-]*)
    echo "Invalid TOGE_DEV_PROFILE '$DEV_PROFILE' (use letters, numbers, dot, underscore, or dash)." >&2
    exit 2
    ;;
esac

DEV_CONFIG_ROOT="${TOGE_DEV_CONFIG_ROOT:-${XDG_CONFIG_HOME:-$HOME/.config}/toge-dev}"
export XDG_CONFIG_HOME="$DEV_CONFIG_ROOT/$DEV_PROFILE"
export XDG_STATE_HOME="$DEV_RUNTIME_DIR/state"
export TOGE_SOCKET="$DEV_RUNTIME_DIR/toged.sock"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME"

echo "Development profile: $DEV_PROFILE"
echo "Development settings: $XDG_CONFIG_HOME/toge/config.toml"
echo "Development state: $XDG_STATE_HOME/toge (removed on exit)"

echo "Starting toged for this dev session..."
"$DAEMON_BIN" --socket "$TOGE_SOCKET" &
PID_DAEMON=$!

echo "Starting Vite dev server..."
cd "$GUI_DIR"
npm run dev &
PID_VITE=$!

# Wait for Vite to be ready
for i in $(seq 1 30); do
  if curl -s http://localhost:1420 > /dev/null 2>&1; then
    break
  fi
  sleep 0.5
done

cd "$SCRIPT_DIR"
cargo run "${CARGO_PROFILE_ARGS[@]}" -p toge-gui-lib
