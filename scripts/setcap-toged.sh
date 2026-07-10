#!/usr/bin/env bash
# Grant fanotify capabilities to the toged daemon binary.
#
# fanotify FAN_MARK_FILESYSTEM requires CAP_SYS_ADMIN, and open_by_handle_at
# (used to resolve file handles to paths) requires CAP_DAC_READ_SEARCH.
# On btrfs, CAP_SYS_ADMIN is also needed to mount the root subvolume.
#
# Usage:
#   sudo ./scripts/setcap-toged.sh [path/to/toged]
#
# If no path is given, defaults to the debug build in target/debug/toged.

set -euo pipefail

BINARY="${1:-$(dirname "$0")/../target/debug/toged}"

if [ ! -f "$BINARY" ]; then
    echo "error: binary not found at $BINARY" >&2
    exit 1
fi

echo "Setting capabilities on: $BINARY"
sudo setcap cap_sys_admin,cap_dac_read_search+ep "$BINARY"

echo "Verifying:"
getcap "$BINARY"

cat <<'EOF'

Done. toged can now use fanotify filesystem-level watches.
Expected result: ~1 fanotify mark per filesystem (btrfs root covers all
subvolumes) instead of ~58,000 inotify watches.
EOF
