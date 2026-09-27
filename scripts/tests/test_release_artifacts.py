"""Verify release archives ship the client and its matching daemon together."""

import hashlib
import pathlib
import subprocess
import tarfile
import tempfile
import unittest


REPO = pathlib.Path(__file__).resolve().parents[2]


class ReleaseArtifactsTest(unittest.TestCase):
    def test_archive_contains_slint_and_matching_tools_for_each_architecture(self):
        for architecture in ("linux-x86_64", "linux-aarch64"):
            with self.subTest(architecture=architecture), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                scripts = root / "scripts" / "release"
                scripts.mkdir(parents=True)
                for name in ("package-artifacts.sh", "lib.sh"):
                    (scripts / name).write_bytes((REPO / "scripts" / "release" / name).read_bytes())
                binaries = root / "target" / "release"
                binaries.mkdir(parents=True)
                for name in ("toge", "toged", "toge-slint"):
                    binary = binaries / name
                    binary.write_text(f"fixture {name}")
                    binary.chmod(0o755)
                for name in ("README.md", "CHANGELOG.md", "LICENSE"):
                    (root / name).write_text(name)
                (root / "toge-slint").mkdir()
                (root / "toge-slint" / "README.md").write_text("Slint usage")
                subprocess.run(
                    ["bash", str(scripts / "package-artifacts.sh"), "v0.2.0", "stable", architecture],
                    cwd=root, check=True, capture_output=True,
                )
                stem = f"toge-v0.2.0-{architecture}"
                archive = root / "dist" / f"{stem}.tar.gz"
                with tarfile.open(archive) as packaged:
                    expected = {"toge", "toged", "toge-slint", "README.md", "README-slint.md", "CHANGELOG.md", "LICENSE"}
                    members = {pathlib.PurePosixPath(m.name).name: m for m in packaged.getmembers() if m.isfile()}
                    self.assertEqual(set(members), expected)
                    for name in ("toge", "toged", "toge-slint"):
                        self.assertTrue(members[name].mode & 0o111)
                        self.assertEqual(packaged.extractfile(members[name]).read(), f"fixture {name}".encode())
                checksum = (root / "dist" / f"{stem}.sha256").read_text().split()[0]
                self.assertEqual(checksum, hashlib.sha256(archive.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
