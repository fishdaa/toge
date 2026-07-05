#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/release/lib.sh
source "$script_dir/lib.sh"

artifact_version="${1:-}"
channel="${2:-stable}"
target_triple="${3:-linux-x86_64}"

if [[ -z "$artifact_version" ]]; then
  printf 'usage: %s <artifact-version> [channel] [target-triple]\n' "${0##*/}" >&2
  exit 1
fi

release_name="needle-${artifact_version}-${target_triple}"
release_dir="release/${release_name}"

rm -rf "$release_dir" dist
mkdir -p "$release_dir" dist
cp target/release/ndl "$release_dir"/
cp target/release/needled "$release_dir"/
cp README.md CHANGELOG.md LICENSE "$release_dir"/
tar -C release -czf "dist/${release_name}.tar.gz" "$(basename "$release_dir")"
sha256sum "dist/${release_name}.tar.gz" >"dist/${release_name}.sha256"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'artifact_version=%s\n' "$artifact_version" >>"$GITHUB_OUTPUT"
  printf 'channel=%s\n' "$channel" >>"$GITHUB_OUTPUT"
  printf 'archive_name=%s\n' "${release_name}.tar.gz" >>"$GITHUB_OUTPUT"
fi
