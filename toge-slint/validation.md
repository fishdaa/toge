# MVP validation

Validated on 2026-09-09, Linux x86_64, Rust 1.97.1, Slint 1.17.1.

## Checks

- Release build: `cargo build --release -p toge-slint -p toged` passed.
- Seven unit/IPC tests passed with local Unix sockets enabled. Covers stale query
  invalidation before debounce, latest pending query replacement, numeric sorting,
  selection lookup, timestamp formatting, incorrect response IDs, malformed,
  oversized and truncated frames, and stalled-peer read timeout.
- `cargo clippy -p toge-slint --all-targets -- -D warnings` passed.
- Rust formatting, `git diff --check`, and launcher Bash syntax check passed.
- Normal dependency tree contains the software renderer and excludes Tauri,
  WebKit and Qt.

## Native window smoke test

Launched the optimized client on Niri/Wayland against an isolated daemon indexing
10,000 temporary text files. Configuration enabled size and modified-date metadata.
The daemon and files were created only for this check and cleaned up afterward.

Inspected a screenshot of the actual 936 × 1014 window: search toolbar and four
columns fill the window, the first row is fully visible, 10,000/10,000 matches are
reported, and the footer remains compact. Slint attribution is available through
a separate About window, created only on demand. The list's standard widget uses
a virtualized ListView; the Rust adapter formats requested rows on demand.

Final smoke sample, approximately three seconds after mapping:

| Metric | Value |
| --- | ---: |
| UI PSS | 24,480 KiB (~23.9 MiB) |
| UI RSS | 37,108 KiB (~36.2 MiB) |
| Daemon PSS | 6,435 KiB (~6.3 MiB) |
| Daemon RSS | 8,440 KiB (~8.2 MiB) |
| Launch until compositor reports window | 68.6 ms |

Memory was read from each process's `/proc/<pid>/smaps_rollup`. The daemon was
launched separately, not counted as part of UI memory. These are individual smoke
samples, not acceptance benchmarks. Compositor registration was polled at 50 ms
intervals and does not measure first painted frame, input readiness, or first
results. No claim of 50% RAM savings or 30% faster startup is established yet.

## Remaining validation

- Matched Tauri comparison with 20 launch trials, median/p95, empty and populated
  windows, cold launch, idle CPU, peak memory and scrolling/input latency.
- Interactive keyboard, clipboard, double-click, About dialog and resize checks
  on both Wayland and X11; current native smoke coverage is Wayland rendering and
  real daemon result loading only.
- Tray, global shortcuts, autostart, settings UI and packaging are outside the MVP.
