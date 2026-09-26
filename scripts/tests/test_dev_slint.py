"""Launcher integration tests with fake builds, capabilities, and Unix sockets.

Run: python3 -m unittest discover -s scripts/tests -v
No root access or capability changes are used.
"""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[2]


class SlintLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="toge launcher ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        for name in ["dev-slint.sh", "check-toged-capabilities.sh"]:
            shutil.copy(REPO / "scripts" / name, self.root / "scripts" / name)
        self.trace = self.root / "trace"
        self.capabilities = self.root / "capabilities"
        mock_bin = self.root / "bin"
        mock_bin.mkdir()
        self.env = dict(os.environ, PATH=f"{mock_bin}:{os.environ['PATH']}",
                        TRACE=str(self.trace), CAPS=str(self.capabilities),
                        TOGE_DEV_CONFIG_ROOT=str(self.root / "config"))
        self.env.pop("TOGE_DEV_PROFILE", None)
        self.write_executable(mock_bin / "cargo", '''
import os, pathlib, sys
with open(os.environ['TRACE'], 'a') as f: f.write('build ' + ' '.join(sys.argv[1:]) + '\\n')
if os.environ.get('RELINK') == '1': pathlib.Path(os.environ['CAPS']).unlink(missing_ok=True)
sys.exit(int(os.environ.get('BUILD_EXIT', '0')))
''')
        self.write_executable(mock_bin / "getcap", '''
import os, pathlib, sys
with open(os.environ['TRACE'], 'a') as f: f.write('check ' + sys.argv[1] + '\\n')
if os.environ.get('GETCAP_EXIT'): sys.exit(int(os.environ['GETCAP_EXIT']))
if pathlib.Path(os.environ['CAPS']).exists(): print(sys.argv[1] + ' cap_dac_read_search,cap_sys_admin=ep')
''')
        for profile in ["debug", "release"]:
            directory = self.root / "target" / profile
            directory.mkdir(parents=True)
            self.write_executable(directory / "toged", '''
import os, socket, sys, time
with open(os.environ['TRACE'], 'a') as f: f.write('daemon\\n')
s = socket.socket(socket.AF_UNIX); s.bind(sys.argv[sys.argv.index('--socket') + 1]); s.listen()
while True: time.sleep(1)
''')
            self.write_executable(directory / "toge-slint", '''
import os, pathlib, sys
if '--request-watcher-access' in sys.argv:
 with open(os.environ['TRACE'], 'a') as f: f.write('access ' + sys.argv[2] + '\\n')
 if os.environ.get('APPROVE') == '1': pathlib.Path(os.environ['CAPS']).touch()
 sys.exit(0 if pathlib.Path(os.environ['CAPS']).exists() else 1)
with open(os.environ['TRACE'], 'a') as f: f.write('gui ' + os.environ['TOGE_SOCKET'] + '\\n')
''')

    def write_executable(self, path, source):
        path.write_text("#!/usr/bin/env python3\n" + source)
        path.chmod(0o755)

    def launch(self, *args):
        return subprocess.run(["bash", str(self.root / "scripts/dev-slint.sh"), *args],
                              env=self.env, capture_output=True, text=True, timeout=10)

    def events(self):
        return self.trace.read_text().splitlines()

    def test_missing_capabilities_stop_before_daemon_start(self):
        result = self.launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(self.events()), 2)
        self.assertTrue(self.events()[0].startswith("build "))
        self.assertTrue(self.events()[1].startswith("access "))

    def test_rebuild_loses_capabilities_and_stops(self):
        self.capabilities.touch()
        self.env["RELINK"] = "1"
        result = self.launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.events()[1].startswith("access "))
        self.assertEqual(len(self.events()), 2)

    def test_verified_build_launches_and_cleans_runtime_state(self):
        self.capabilities.touch()
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        events = self.events()
        self.assertEqual(events[3], "daemon")
        self.assertTrue(events[4].startswith("gui "))
        self.assertFalse(Path(events[4][4:]).parent.exists())

    def test_release_checks_release_executable(self):
        result = self.launch("--release")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("build build --release", self.events()[0])
        self.assertIn("target/release/toged", self.events()[1])
        self.assertTrue(self.events()[1].startswith("access "))

    def test_build_failure_never_checks_or_launches(self):
        self.env["BUILD_EXIT"] = "7"
        result = self.launch()
        self.assertEqual(result.returncode, 7)
        self.assertEqual(len(self.events()), 1)

    def test_capability_inspection_failure_stops(self):
        self.capabilities.touch()
        self.env["GETCAP_EXIT"] = "3"
        result = self.launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("could not inspect", result.stderr)
        self.assertEqual(len(self.events()), 3)

    def test_explicit_approval_is_verified_before_launch(self):
        self.env["APPROVE"] = "1"
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        events = self.events()
        self.assertTrue(events[1].startswith("access "))
        self.assertTrue(events[2].startswith("check "))
        self.assertEqual(events[3], "daemon")


if __name__ == "__main__":
    unittest.main()
