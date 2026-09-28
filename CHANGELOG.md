# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and the project follows Semantic Versioning.

## [Unreleased]

## [0.2.1] - 2026-09-28

### Fixed

- fix: reject unrepresentable index paths and enable pedantic clippy lints (#36)


## [0.2.0] - 2026-09-27

### Added

- Experimental Slint Linux desktop client with paged search results, keyboard navigation, file actions, preferences, multiple windows, and tray integration.
- Daemon result sessions with progressive previews, bounded page fetching, cancellation, and reconciliation after filesystem changes.
- Slint binary and usage guide in x86_64 and ARM64 release archives, alongside the matching CLI and daemon.

### Fixed

- Instance isolation for multiple daemon sockets and large selection actions beyond the display cache.
- Search and sorting performance, metadata refresh, and daemon readiness handling.
- Removed stale indexed descendants when a configured root is deleted or moved.
- Prevented stalled preview readers from holding the shared index lock.
- Made concurrent index saves use separate temporary files so published indexes remain complete.

### Changed

- Updated serde to 1.0.229 (#28), serde_json to 1.0.151 (#29), libc to 0.2.189 (#30), time to 0.3.55 (#33), and Tauri to 2.11.6 (#34).
- Updated the stale-issue action to v11 (#32).
- CLI and daemon version output now follows the compiled workspace version.

### Notes

- Tauri installer packages remain available; the Slint client is distributed in the binary archive.
- Keep the bundled daemon beside the Slint executable. Older daemons do not support result sessions.


## [0.1.16] - 2026-07-21

### Fixed

- Automation/release v0.1.12 (#20)

### Changed

- chore(deps): Bump regex from 1.13.0 to 1.13.1 (#26)

## [0.1.15] - 2026-07-13

### Fixed

- Automation/release v0.1.12 (#20)


## [0.1.14] - 2026-07-13

### Fixed

- Automation/release v0.1.12 (#20)

### Changed

- chore(deps): Bump regex from 1.12.4 to 1.13.0 (#19)


## [0.1.13] - 2026-07-13

### Fixed

- Automation/release v0.1.12 (#20)

### Changed

- chore(deps): Bump regex from 1.12.4 to 1.13.0 (#19)


## [0.1.12] - 2026-07-10

### Fixed

- chore(release): update version to 0.1.8 and adjust release workflow (#14)


## [0.1.11] - 2026-07-06

### Fixed

- chore(release): update version to 0.1.8 and adjust release workflow (#14)


## [0.1.10] - 2026-07-06

### Fixed

- chore(release): update version to 0.1.8 and adjust release workflow (#14)
- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.9] - 2026-07-06

### Fixed

- chore(release): update version to 0.1.8 and adjust release workflow (#14)
- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.7] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.6] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.5] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.4] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)


## [0.1.3] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)
- chore(deps): Bump actions/checkout from 4 to 7 (#1)


## [0.1.2] - 2026-07-05

### Fixed

- Automation/release v0.1.1 (#7)
- fix: Fixed `needled` readiness semantics to ensure queries fail until… (#6)

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)
- chore(deps): Bump actions/checkout from 4 to 7 (#1)


## [0.1.1] - 2026-07-05

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)
- chore(deps): Bump actions/checkout from 4 to 7 (#1)


- Initial open source project scaffolding

## [0.1.1] - 2026-07-05

- Fixed `needled` readiness semantics so queries fail until the initial index is ready, and taught `ndl` to wait for daemon readiness before issuing search requests
- Moved daemon reindex work out of the global state mutex to avoid blocking all requests during full rebuilds
- Fixed Linux inotify path resolution to use watch descriptors directly, which corrects delete handling and duplicate-basename collisions across watched directories
- Added watcher health reporting to daemon status, including watch coverage, watch failures, and inotify overflow counts
- Triggered full daemon reindex after inotify overflow so stale watcher state is repaired instead of silently persisting
- Reduced watcher startup lock contention and expanded tests around daemon readiness and wd-based watcher resolution

## [0.1.0] - 2026-07-03

- Initial public workspace structure for `needle-core`, `needled`, and `ndl`
