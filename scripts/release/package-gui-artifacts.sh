#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/release/lib.sh
source "$script_dir/lib.sh"

artifact_version="${1:-}"
if [[ -z "$artifact_version" ]]; then
  printf 'usage: %s <artifact-version>\n' "${0##*/}" >&2
  exit 1
fi
artifact_version="${artifact_version#v}"
assert_clean_version "$artifact_version"

repo="$(repo_root)"
bundle_dir="$repo/target/release/bundle"
dist_dir="$repo/dist"

cargo build --release -p toge -p toged
(
  cd "$repo/toge-gui"
  npm run tauri build -- --bundles deb,rpm,appimage
)

copy_one() {
  local pattern="$1"
  local destination="$2"
  local source
  source="$(find "$bundle_dir" -type f -path "$pattern" -print -quit)"
  if [[ -z "$source" ]]; then
    printf 'missing GUI bundle matching %s\n' "$pattern" >&2
    exit 1
  fi
  cp "$source" "$dist_dir/$destination"
}

mkdir -p "$dist_dir"
copy_one '*/deb/*.deb' "toge-gui_${artifact_version}_amd64.deb"
copy_one '*/rpm/*.rpm' "toge-gui-${artifact_version}-1.x86_64.rpm"
copy_one '*/appimage/*.AppImage' "Toge_${artifact_version}_amd64.AppImage"

artifacts=(
  "$dist_dir/toge-gui_${artifact_version}_amd64.deb"
  "$dist_dir/toge-gui-${artifact_version}-1.x86_64.rpm"
  "$dist_dir/Toge_${artifact_version}_amd64.AppImage"
)
for artifact in "${artifacts[@]}"; do
  sha256sum "$artifact" >"$artifact.sha256"
done
