### Added

- Experimental Slint Linux desktop client with paged search results, keyboard navigation, file actions, preferences, multiple windows, and tray integration.
- Daemon result sessions with progressive previews, bounded page fetching, cancellation, and reconciliation after filesystem changes.
- Slint binary and usage guide in x86_64 and ARM64 release archives, alongside the matching CLI and daemon.

### Fixed

- Instance isolation for multiple daemon sockets and large selection actions beyond the display cache.
- Search and sorting performance, metadata refresh, and daemon readiness handling.

### Notes

- Tauri installer packages remain available; the Slint client is distributed in the binary archive.
- Keep the bundled daemon beside the Slint executable. Older daemons do not support result sessions.
