#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/release/lib.sh
source "$script_dir/lib.sh"

version="${1:-}"
if [[ -z "$version" ]]; then
  printf 'usage: %s <version>\n' "${0##*/}" >&2
  exit 1
fi

assert_clean_version "$version"

manifest="$(workspace_manifest)"

python3 - "$manifest" "$version" <<'PY'
import pathlib
import re
import sys

manifest = pathlib.Path(sys.argv[1])
version = sys.argv[2]
text = manifest.read_text()

workspace_package_pattern = re.compile(
    r'(\[workspace\.package\][^\[]*?^version = ")([^"]+)(")',
    re.MULTILINE | re.DOTALL,
)
workspace_dependency_pattern = re.compile(
    r'(\[workspace\.dependencies\][^\[]*?^toge-core = \{[^}]*\bversion = ")([^"]+)(")',
    re.MULTILINE | re.DOTALL,
)

text, package_count = workspace_package_pattern.subn(r"\g<1>" + version + r"\g<3>", text, count=1)
text, dependency_count = workspace_dependency_pattern.subn(r"\g<1>" + version + r"\g<3>", text, count=1)

if package_count != 1 or dependency_count != 1:
    raise SystemExit("failed to update workspace version fields")

manifest.write_text(text)
PY

cargo metadata --format-version 1 --no-deps >/dev/null
