#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Building frontend..."
cd "$SCRIPT_DIR/toge-gui"
npm run build

cd "$SCRIPT_DIR"
cargo run --release -p toge-gui-lib
