"""Launcher integration tests with fake builds, capabilities, and Unix sockets.

Run: python3 -m unittest discover -s scripts/tests -v
No root access or capability changes are used.
"""
import fcntl
import json
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
        self.details = self.root / "launch-details"
        self.state_root = self.root / "state"
        mock_bin = self.root / "bin"
        mock_bin.mkdir()
        self.env = dict(os.environ, PATH=f"{mock_bin}:{os.environ['PATH']}",
                        TRACE=str(self.trace), CAPS=str(self.capabilities),
                        TOGE_DEV_CONFIG_ROOT=str(self.root / "config"),
                        TOGE_DEV_STATE_ROOT=str(self.state_root),
                        LAUNCH_DETAILS=str(self.details))
        self.env.pop("TOGE_DEV_PROFILE", None)
        self.env.pop("CARGO_TARGET_DIR", None)
        self.write_executable(mock_bin / "cargo", '''
import json, os, pathlib, sys
with open(os.environ['TRACE'], 'a') as f: f.write('build ' + ' '.join(sys.argv[1:]) + '\\n')
if os.environ.get('RELINK') == '1': pathlib.Path(os.environ['CAPS']).unlink(missing_ok=True)
status = int(os.environ.get('BUILD_EXIT', '0'))
if status: sys.exit(status)
profile = 'release' if '--release' in sys.argv else 'debug'
target = pathlib.Path(os.environ.get('CARGO_TARGET_DIR', str(pathlib.Path.cwd() / 'target')))
for name in ['toge-slint', 'toged']:
 if os.environ.get('MISSING_ARTIFACT') == name: continue
 print(json.dumps({'reason': 'compiler-artifact', 'target': {'name': name, 'kind': ['bin']},
                   'executable': str(target / profile / name), 'fresh': True}))
print(json.dumps({'reason': 'build-finished', 'success': True}))
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
import json, os, pathlib, socket, sys, time
state = pathlib.Path(os.environ['XDG_STATE_HOME']) / 'toge'
index = state / 'index.bin'
with open(os.environ['LAUNCH_DETAILS'], 'a') as f:
 f.write(json.dumps({'state': str(state), 'cached': index.exists(),
                     'inherited_lock': os.path.exists('/proc/self/fd/9')}) + '\\n')
state.mkdir(parents=True, exist_ok=True)
index.write_bytes(b'saved development index')
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

    def test_verified_build_keeps_index_and_cleans_runtime_socket(self):
        self.capabilities.touch()
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        events = self.events()
        self.assertEqual(events[3], "daemon")
        self.assertTrue(events[4].startswith("gui "))
        self.assertFalse(Path(events[4][4:]).parent.exists())
        self.assertTrue((self.state_root / "slint/toge/index.bin").exists())
        self.assertFalse(self.launches()[0]["inherited_lock"])

    def launches(self):
        return [json.loads(line) for line in self.details.read_text().splitlines()]

    def test_second_release_launch_reuses_saved_index_with_a_new_socket(self):
        self.capabilities.touch()
        sockets = []
        for _ in range(2):
            result = self.launch("--release")
            self.assertEqual(result.returncode, 0, result.stderr)
            sockets.append(Path(self.events()[-1][4:]))
        launches = self.launches()
        self.assertEqual([launch["cached"] for launch in launches], [False, True])
        self.assertEqual(launches[0]["state"], launches[1]["state"])
        self.assertNotEqual(sockets[0], sockets[1])
        self.assertTrue(all(not socket.parent.exists() for socket in sockets))

    def test_debug_and_release_share_state_within_a_profile(self):
        self.capabilities.touch()
        for args in [(), ("--release",)]:
            result = self.launch(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([launch["cached"] for launch in self.launches()], [False, True])

    def test_profiles_keep_separate_saved_indexes(self):
        self.capabilities.touch()
        for profile in ["slint", "other", "slint"]:
            self.env["TOGE_DEV_PROFILE"] = profile
            result = self.launch()
            self.assertEqual(result.returncode, 0, result.stderr)
        launches = self.launches()
        self.assertEqual([launch["cached"] for launch in launches], [False, False, True])
        self.assertNotEqual(launches[0]["state"], launches[1]["state"])
        self.assertEqual(launches[0]["state"], launches[2]["state"])

    def test_state_defaults_to_xdg_state_home(self):
        self.capabilities.touch()
        self.env.pop("TOGE_DEV_STATE_ROOT")
        self.env["XDG_STATE_HOME"] = str(self.root / "xdg-state")
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(self.launches()[0]["state"]),
                         self.root / "xdg-state/toge-dev/slint/toge")

    def test_state_falls_back_to_home_when_xdg_state_home_is_unset(self):
        self.capabilities.touch()
        self.env.pop("TOGE_DEV_STATE_ROOT")
        self.env.pop("XDG_STATE_HOME", None)
        self.env["HOME"] = str(self.root / "home")
        result = self.launch()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(Path(self.launches()[0]["state"]),
                         self.root / "home/.local/state/toge-dev/slint/toge")

    def test_busy_profile_stops_before_starting_a_second_daemon(self):
        self.capabilities.touch()
        state = self.state_root / "slint"
        state.mkdir(parents=True)
        with (state / "launcher.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result = self.launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already running", result.stderr)
        self.assertNotIn("daemon", self.events())
        self.assertFalse(self.details.exists())

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

    def test_custom_target_directory_never_launches_stale_default_binaries(self):
        custom = self.root / "custom target" / "host-triple"
        shutil.copytree(self.root / "target", custom)
        self.env["CARGO_TARGET_DIR"] = str(custom)
        self.capabilities.touch()
        for profile, args in [("debug", ()), ("release", ("--release",))]:
            for name in ["toge-slint", "toged"]:
                self.write_executable(self.root / "target" / profile / name, "raise SystemExit('stale binary launched')")
            self.trace.unlink(missing_ok=True)
            result = self.launch(*args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(str(custom / profile / "toged"), self.events()[1])
            self.assertIn(str(custom / profile / "toge-slint"), result.stdout)
            self.assertEqual(self.events()[3], "daemon")
            self.assertTrue(self.events()[4].startswith("gui "))

    def test_missing_build_artifact_never_falls_back_to_existing_binary(self):
        self.env["MISSING_ARTIFACT"] = "toge-slint"
        self.capabilities.touch()
        result = self.launch()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing to launch an old build", result.stderr)
        self.assertEqual(len(self.events()), 1)


if __name__ == "__main__":
    unittest.main()
