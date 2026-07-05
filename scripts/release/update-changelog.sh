#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/release/lib.sh
source "$script_dir/lib.sh"

version="${1:-}"
notes_file="${2:-}"
date_value="${3:-$(release_date)}"

if [[ -z "$version" || -z "$notes_file" ]]; then
  printf 'usage: %s <version> <notes-file> [date]\n' "${0##*/}" >&2
  exit 1
fi

assert_clean_version "$version"

python3 - "$version" "$notes_file" "$date_value" <<'PY'
import pathlib
import sys

version = sys.argv[1]
notes_path = pathlib.Path(sys.argv[2])
date_value = sys.argv[3]
changelog_path = pathlib.Path("CHANGELOG.md")

notes = notes_path.read_text().strip()
if not notes:
    notes = "- No notable release notes were captured for this release."

new_section = f"## [{version}] - {date_value}\n\n{notes}\n\n"

text = changelog_path.read_text()
marker = "## [Unreleased]\n"
if marker not in text:
    raise SystemExit("missing Unreleased section in CHANGELOG.md")

if f"## [{version}] - " in text:
    raise SystemExit(f"CHANGELOG.md already contains {version}")

updated = text.replace(marker, marker + "\n" + new_section, 1)
changelog_path.write_text(updated)
PY

