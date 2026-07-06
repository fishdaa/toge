# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and the project follows Semantic Versioning.

## [Unreleased]

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
