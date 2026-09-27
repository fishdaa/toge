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
