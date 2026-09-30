#!/usr/bin/env python3
"""Toggle the active Slint development window from a compositor key binding.

The development launcher gives each run a temporary daemon socket, so a
compositor cannot use a fixed ``TOGE_SOCKET`` value. Find the process for the
selected development profile and reuse its socket and executable.
"""

import os
import pathlib
import subprocess
import sys


def main() -> int:
    profile = os.environ.get("TOGE_DEV_PROFILE", "slint")
    config_root = pathlib.Path(
        os.environ.get(
            "TOGE_DEV_CONFIG_ROOT",
            pathlib.Path(os.environ.get("XDG_CONFIG_HOME", pathlib.Path.home() / ".config"))
            / "toge-dev",
        )
    )
    config_home = str(config_root / profile)
    for proc in pathlib.Path("/proc").iterdir():
        if not proc.name.isdecimal():
            continue
        try:
            if proc.joinpath("comm").read_text().strip() != "toge-slint":
                continue
            entries = proc.joinpath("environ").read_bytes().split(b"\0")
            environment = dict(entry.split(b"=", 1) for entry in entries if b"=" in entry)
            if os.fsdecode(environment.get(b"XDG_CONFIG_HOME", b"")) != config_home:
                continue
            socket = os.fsdecode(environment[b"TOGE_SOCKET"])
            executable = str(proc / "exe")
        except (OSError, KeyError):
            continue
        return subprocess.call([executable, "--toggle"], env={**os.environ, "TOGE_SOCKET": socket})
    print(f"No running Toge Slint development profile: {profile}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
