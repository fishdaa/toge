#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$SCRIPT_DIR/toge-gui"
PID_VITE=""
PID_DAEMON=""
DEV_RUNTIME_DIR=""

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

echo "Building daemon for this dev session..."
cd "$SCRIPT_DIR"
cargo build -p toged
sudo setcap cap_sys_admin,cap_dac_read_search+ep "$SCRIPT_DIR/target/debug/toged"

DEV_RUNTIME_DIR="$(mktemp -d /tmp/toge-gui-dev.XXXXXX)"
export TOGE_SOCKET="$DEV_RUNTIME_DIR/toged.sock"

echo "Starting toged for this dev session..."
"$SCRIPT_DIR/target/debug/toged" --socket "$TOGE_SOCKET" &
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
cargo run -p toge-gui-lib
