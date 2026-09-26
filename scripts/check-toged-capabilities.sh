#!/usr/bin/env bash
# Read-only preflight for filesystem-wide fanotify support. Run after Cargo,
# since relinking the executable removes previously granted capabilities.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "Usage: $0 path/to/toged" >&2; exit 2; }
BINARY="$1"
[ -f "$BINARY" ] && [ -x "$BINARY" ] || {
    echo "error: executable not found at $BINARY" >&2
    exit 1
}
command -v getcap >/dev/null || {
    echo "error: install libcap tools (getcap and setcap) before starting Slint" >&2
    exit 1
}
CAPABILITIES="$(getcap "$BINARY")" || {
    echo "error: could not inspect fanotify capabilities on $BINARY" >&2
    exit 1
}
REQUIRED='cap_dac_read_search,cap_sys_admin=ep'
if [ "$CAPABILITIES" != "$BINARY $REQUIRED" ]; then
    echo "error: required fanotify capabilities are missing on $BINARY" >&2
    echo "Cargo relinking removes file capabilities. Apply them after building:" >&2
    printf '  sudo setcap %q %q\n' "$REQUIRED" "$BINARY" >&2
    echo "Or rerun the Slint launch command to request access in the approval dialog." >&2
    exit 1
fi
echo "Fanotify capabilities verified: $BINARY"
