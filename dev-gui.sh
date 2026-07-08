#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GUI_DIR="$SCRIPT_DIR/toge-gui"
PID_VITE=""

cleanup() {
  if [ -n "$PID_VITE" ] && kill -0 "$PID_VITE" 2>/dev/null; then
    kill "$PID_VITE" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

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
