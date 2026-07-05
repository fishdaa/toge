#!/usr/bin/env bash

set -euo pipefail

base_version="${1:-}"
if [[ -z "$base_version" ]]; then
  printf 'usage: %s <base-version>\n' "${0##*/}" >&2
  exit 1
fi

latest="$(git tag --list "v${base_version}-beta.*" --sort=version:refname | tail -n1)"
if [[ -z "$latest" ]]; then
  next_number=1
else
  next_number="${latest##*.}"
  next_number=$((next_number + 1))
fi

printf 'v%s-beta.%s\n' "$base_version" "$next_number"

