#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/release/lib.sh
source "$script_dir/lib.sh"

tag="${1:-}"
if [[ -z "$tag" ]]; then
  printf 'usage: %s <tag>\n' "${0##*/}" >&2
  exit 1
fi

if [[ ! "$tag" =~ ^v(.+)$ ]]; then
  printf 'tag must start with v: %s\n' "$tag" >&2
  exit 1
fi

tag_version="${BASH_REMATCH[1]}"
manifest_version="$(current_workspace_version)"

if [[ "$tag_version" != "$manifest_version" ]]; then
  printf 'tag version %s does not match workspace version %s\n' "$tag_version" "$manifest_version" >&2
  exit 1
fi

