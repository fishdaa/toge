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

machine="${TOGE_PACKAGE_ARCH:-$(uname -m)}"
case "$machine" in
  x86_64 | amd64)
    deb_arch="amd64"
    rpm_arch="x86_64"
    appimage_arch="amd64"
    ;;
  aarch64 | arm64)
    deb_arch="arm64"
    rpm_arch="aarch64"
    appimage_arch="aarch64"
    ;;
  *)
    printf 'unsupported package architecture: %s\n' "$machine" >&2
    exit 1
    ;;
esac

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
copy_one '*/deb/*.deb' "toge-gui_${artifact_version}_${deb_arch}.deb"
copy_one '*/rpm/*.rpm' "toge-gui-${artifact_version}-1.${rpm_arch}.rpm"
copy_one '*/appimage/*.AppImage' "Toge_${artifact_version}_${appimage_arch}.AppImage"

artifacts=(
  "$dist_dir/toge-gui_${artifact_version}_${deb_arch}.deb"
  "$dist_dir/toge-gui-${artifact_version}-1.${rpm_arch}.rpm"
  "$dist_dir/Toge_${artifact_version}_${appimage_arch}.AppImage"
)
for artifact in "${artifacts[@]}"; do
  sha256sum "$artifact" >"$artifact.sha256"
done
