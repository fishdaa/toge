#!/bin/sh

set -eu

if ! command -v setcap >/dev/null 2>&1; then
    echo "toge: setcap is required to enable the fanotify filesystem watcher" >&2
    exit 1
fi

setcap cap_sys_admin,cap_dac_read_search+ep /usr/bin/toged
