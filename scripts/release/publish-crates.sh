#!/usr/bin/env bash

set -euo pipefail

channel="${1:-stable}"

extra_args=(--locked)
if [[ "$channel" == "beta" ]]; then
  extra_args+=(--allow-dirty)
fi

cargo publish -p needle-core "${extra_args[@]}"
sleep 10
cargo publish -p ndl "${extra_args[@]}"
sleep 10
cargo publish -p needled "${extra_args[@]}"
