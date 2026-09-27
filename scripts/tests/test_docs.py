"""Catch documentation drift that can be checked mechanically."""

import os
import pathlib
import re
import subprocess
import tempfile
import unittest


REPO = pathlib.Path(__file__).resolve().parents[2]


def tracked_markdown():
    listed = subprocess.run(
        ["git", "ls-files", "*.md"], cwd=REPO, check=True, capture_output=True, text=True
    ).stdout.split()
    return [REPO / name for name in listed]


def workspace_version():
    manifest = (REPO / "Cargo.toml").read_text()
    return re.search(r'\[workspace\.package\][\s\S]*?^version = "([^"]+)"', manifest, re.M).group(1)


def usage_lines(source):
    """Lines printed by a crate's `fn usage()`, without blank lines."""
    body = re.search(r"fn usage\(\) \{\n(.*?)\n\}", source.read_text(), re.S).group(1)
    return [line for line in re.findall(r'println!\("(.*)"\);', body) if line.strip()]


def readme_cli_block(readme):
    """Non-blank lines of the first ```text block under `## CLI`."""
    text = readme.read_text()
    block = re.search(r"^## CLI\n.*?```text\n(.*?)```", text, re.S | re.M).group(1)
    return [line for line in block.splitlines() if line.strip()]


class DocsTest(unittest.TestCase):
    def test_relative_links_resolve_to_tracked_files(self):
        for doc in tracked_markdown():
            text = re.sub(r"```.*?```", "", doc.read_text(), flags=re.S)
            for target in re.findall(r"\]\(([^)\s]+)\)", text):
                if re.match(r"[a-z]+:", target) or target.startswith("#"):
                    continue
                path = (doc.parent / target.split("#", 1)[0]).resolve()
                with self.subTest(doc=str(doc.relative_to(REPO)), link=target):
                    self.assertTrue(path.exists(), "link target does not exist")
                    ignored = subprocess.run(
                        ["git", "check-ignore", "-q", str(path)], cwd=REPO
                    ).returncode == 0
                    self.assertFalse(ignored, "link target is gitignored")

    def test_cli_help_matches_readmes(self):
        for crate in ("toge", "toged"):
            with self.subTest(crate=crate):
                self.assertEqual(
                    readme_cli_block(REPO / crate / "README.md"),
                    usage_lines(REPO / crate / "src" / "main.rs"),
                )

    def test_every_parsed_cli_flag_is_documented(self):
        opts = (REPO / "toge-core" / "src" / "opts.rs").read_text()
        parser = opts[opts.index("let flag = arg.trim_start_matches('-');"):]
        parser = parser[: parser.index("_ => return Err")]
        flags = set()
        for arm in re.findall(r'^\s*((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>', parser, re.M):
            flags.update(re.findall(r'"([^"]+)"', arm))
        readme = (REPO / "toge" / "README.md").read_text()
        for flag in sorted(flags):
            with self.subTest(flag=flag):
                self.assertRegex(readme, rf"(?<![\w-])--?{re.escape(flag)}(?![\w-])")

    def test_release_notes_match_changelog_for_current_version(self):
        version = workspace_version()
        changelog = (REPO / "CHANGELOG.md").read_text()
        section = re.search(
            rf"^## \[{re.escape(version)}\] - \S+\n\n(.*?)(?=^## \[|^\[Unreleased\]:|\Z)",
            changelog,
            re.S | re.M,
        )
        self.assertIsNotNone(section, f"CHANGELOG.md has no {version} section")
        self.assertEqual(
            section.group(1).strip(), (REPO / "release-notes.md").read_text().strip()
        )

    def test_changelog_versions_are_unique_and_linked(self):
        changelog = (REPO / "CHANGELOG.md").read_text()
        versions = re.findall(r"^## \[([^\]]+)\]", changelog, re.M)
        self.assertEqual(len(versions), len(set(versions)), "duplicate changelog sections")
        self.assertIn(f"compare/v{workspace_version()}...HEAD", changelog)


class UpdateChangelogTest(unittest.TestCase):
    def test_new_release_updates_compare_links(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-09-27\n\n- Old\n\n"
                "[Unreleased]: https://example.test/r/compare/v0.2.0...HEAD\n"
                "[0.2.0]: https://example.test/r/compare/v0.1.16...v0.2.0\n"
            )
            (root / "notes.md").write_text("### Fixed\n\n- New\n")
            subprocess.run(
                ["bash", str(REPO / "scripts/release/update-changelog.sh"), "0.2.1", "notes.md", "2026-10-01"],
                cwd=root, check=True, capture_output=True, env={**os.environ},
            )
            text = (root / "CHANGELOG.md").read_text()
            self.assertIn("## [Unreleased]\n\n## [0.2.1] - 2026-10-01\n\n### Fixed\n\n- New\n", text)
            self.assertIn(
                "[Unreleased]: https://example.test/r/compare/v0.2.1...HEAD\n"
                "[0.2.1]: https://example.test/r/compare/v0.2.0...v0.2.1\n"
                "[0.2.0]: https://example.test/r/compare/v0.1.16...v0.2.0\n",
                text,
            )


if __name__ == "__main__":
    unittest.main()
