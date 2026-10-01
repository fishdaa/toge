# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog and the project follows Semantic Versioning.

## [Unreleased]

## [0.2.3] - 2026-10-01

### Fixed

- Fix search filters and add synchronized Slint search controls (#40)
- Feature/noctalia plugin (#37)


## [0.2.2] - 2026-10-01

### Added

- Slint search controls for case, whole-word, path, and regex matching, plus
  file-type presets that edit the raw query and stay in sync with typed modifiers.
- Immediate-parent and depth query filters, quoted filter values, and hidden-name
  attribute matching.
- CLI JSON Lines output (`--json`/`-jsonl`), nonblocking readiness checks
  (`--no-wait`), and `--` to separate query text from options.
- Noctalia launcher plugin with file search, daemon progress, index rebuilding,
  and an optional action to open the Slint client.
- Slint Options window for recording, clearing, and saving application shortcuts,
  plus systemwide window shortcuts through the desktop Global Shortcuts portal.
- Selected-file previews for images, SVG, PDFs, text/code, office documents,
  spreadsheets, audio metadata/artwork, and sampled video stills using optional
  installed tools; reuse of valid freedesktop thumbnails.

### Changed

- PDF previews render at the pane's physical pixel width, load the visible page
  before neighboring pages, and support scrolling through the document.
- Text/code previews show line numbers, wrapping controls, horizontal scrolling,
  keyboard navigation, copying, and optional syntax colors from `bat`/`batcat`.

### Fixed

- Recognized unsupported search filters and attributes report errors instead of
  silently accepting them; regex matching respects the case setting.
- A leading `!` negates search terms instead of matching it literally.
- Extension filters and file-type presets match uppercase extensions, including
  restored indexes; narrow Slint windows retain horizontal result scrolling.
- Video previews display the first frame while samples load and keep it visible
  if duration inspection or sampling fails.
- Slint keeps selected-file details and a compact result summary on one footer
  row, eliding long details to keep status visible.

### Removed

- The unused `exclude_fstypes` (`[roots]`) and `interval_secs` (`[polling]`) config options. Existing config files that set them still load; the values are ignored.
- The CLI flags `-pause`/`-more` and `-config`, which were parsed but had no effect. Passing them is now an unknown-flag error.
- The preview Support button, its popup, and unused diagnostic summary.

## [0.2.1] - 2026-09-28

### Fixed

- fix: reject unrepresentable index paths and enable pedantic clippy lints (#36)

## [0.2.0] - 2026-09-27

### Added

- Experimental Slint Linux desktop client with paged search results, keyboard navigation, file actions (including trash and permanent delete), preferences, multiple windows, and tray integration.
- Daemon result sessions with progressive previews, bounded page fetching, cancellation, and reconciliation after filesystem changes.
- Slint binary and usage guide in x86_64 and ARM64 release archives, alongside the matching CLI and daemon.
- CLI `--stream` for streaming results in index or sorted order with final totals.
- Live-updates access prompt and `scripts/setcap-toged.sh` for granting the daemon's fanotify capabilities.

### Fixed

- Instance isolation for multiple daemon sockets and large selection actions beyond the display cache.
- Search and sorting performance, metadata refresh, and daemon readiness handling.
- Removed stale indexed descendants when a configured root is deleted or moved.
- Prevented stalled preview readers from holding the shared index lock.
- Made concurrent index saves use separate temporary files so published indexes remain complete.

### Changed

- Updated serde to 1.0.229 (#28), serde_json to 1.0.151 (#29), libc to 0.2.189 (#30), and time to 0.3.55 (#33).
- Updated the stale-issue action to v11 (#32).
- CLI and daemon version output now follows the compiled workspace version.

### Removed

- The Tauri desktop GUI and its DEB, RPM, and AppImage packages. The Slint client in the binary archive replaces it; built-in global shortcuts, autostart, settings editing, and diagnostics are not yet available there.

### Notes

- Keep the bundled daemon beside the Slint executable. Older daemons do not support result sessions.

## [0.1.16] - 2026-07-21

### Changed

- chore(deps): Bump regex from 1.13.0 to 1.13.1 (#26)

## [0.1.15] - 2026-07-13

- No notable changes; this release only carried release automation updates.

## [0.1.14] - 2026-07-13

- No notable changes; this release only carried release automation updates.

## [0.1.13] - 2026-07-13

- No notable changes; this release only carried release automation updates.

## [0.1.12] - 2026-07-13

### Changed

- chore(deps): Bump regex from 1.12.4 to 1.13.0 (#19)

This version was released without a Git tag.

## [0.1.11] - 2026-07-10

- No notable changes; this release only carried release automation updates.

## [0.1.10] - 2026-07-06

- No notable changes; this release only carried release automation updates.

## [0.1.9] - 2026-07-06

- No notable changes; this release only carried release automation updates.

## [0.1.8] - 2026-07-06

### Changed

- Adjusted the release workflow (#14)

This version was released without a Git tag.

## [0.1.7] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.6] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.5] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.4] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.3] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.2] - 2026-07-05

- No notable changes; this release only carried release automation updates.

## [0.1.1] - 2026-07-05

### Changed

- chore(deps): Bump peter-evans/create-pull-request from 7 to 8 (#4)
- chore(deps): Bump actions/github-script from 8 to 9 (#3)
- chore(deps): Bump softprops/action-gh-release from 2 to 3 (#2)
- chore(deps): Bump actions/checkout from 4 to 7 (#1)

### Fixed

- Fixed `needled` readiness semantics so queries fail until the initial index is ready, and taught `ndl` to wait for daemon readiness before issuing search requests
- Moved daemon reindex work out of the global state mutex to avoid blocking all requests during full rebuilds
- Fixed Linux inotify path resolution to use watch descriptors directly, which corrects delete handling and duplicate-basename collisions across watched directories
- Added watcher health reporting to daemon status, including watch coverage, watch failures, and inotify overflow counts
- Triggered full daemon reindex after inotify overflow so stale watcher state is repaired instead of silently persisting
- Reduced watcher startup lock contention and expanded tests around daemon readiness and wd-based watcher resolution

## [0.1.0] - 2026-07-03

- Initial public workspace structure for `needle-core`, `needled`, and `ndl`
- Initial open source project scaffolding

[Unreleased]: https://github.com/fishdaa/needle/compare/v0.2.3...HEAD
[0.2.3]: https://github.com/fishdaa/needle/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/fishdaa/needle/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/fishdaa/needle/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/fishdaa/needle/compare/v0.1.16...v0.2.0
[0.1.16]: https://github.com/fishdaa/needle/compare/v0.1.15...v0.1.16
[0.1.15]: https://github.com/fishdaa/needle/compare/v0.1.14...v0.1.15
[0.1.14]: https://github.com/fishdaa/needle/compare/v0.1.13...v0.1.14
[0.1.13]: https://github.com/fishdaa/needle/compare/v0.1.11...v0.1.13
[0.1.11]: https://github.com/fishdaa/needle/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/fishdaa/needle/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/fishdaa/needle/compare/v0.1.7...v0.1.9
[0.1.7]: https://github.com/fishdaa/needle/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/fishdaa/needle/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/fishdaa/needle/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/fishdaa/needle/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/fishdaa/needle/releases/tag/v0.1.3
