#!/usr/bin/env bash

set -euo pipefail

repo_root() {
  git rev-parse --show-toplevel
}

workspace_manifest() {
  printf '%s/Cargo.toml\n' "$(repo_root)"
}

current_workspace_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$(workspace_manifest)" | head -n1
}

assert_clean_version() {
  local version="$1"
  if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.+-]+)?$ ]]; then
    printf 'invalid version: %s\n' "$version" >&2
    exit 1
  fi
}

release_date() {
  date -u +%F
}

