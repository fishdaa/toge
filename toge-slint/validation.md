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

## Daemon stream integration — 2026-09-27

- All 26 Slint unit/IPC tests passed with `--test-threads=1`; strict Clippy,
  formatting, and native app/daemon builds passed. An unrelated temporary-script
  access test failed once in the parallel run and passed individually and in
  subsequent serial suites.
- The client requests `StreamQuery` in index order, publishes bounded 128-row
  batches, and updates exact totals only after the completion summary. Tests cover
  progressive delivery before completion, 25,003 rows, incorrect IDs, incomplete
  streams, empty/error responses, older-daemon errors, and socket cancellation.
- Native Niri/Wayland verification used the shared `ui/main.slint` and actual
  client, worker, and model against a throttled stream fixture with 4,096 rows.
  Injected Slint WindowEvents exercised keyboard input, focus, scrolling,
  selection, cancellation, late-arriving selected paths, column sorting, empty
  results, query errors, and recovery. The final run kept the window unfocused to
  avoid interference with desktop keyboard input.
- Captured frames were inspected for clipping, focus, scroll position, loading
  status, and visible errors. A late-restored selection initially scrolled out of
  view as batches changed the table geometry; final completion now repositions
  that restored selection, and the passing recording shows it visibly selected.
- Recording and assertions are outside version control under
  `/tmp/toge-slint-stream-recording/`. The native recording is available only on
  the tailnet at
  [daemon-stream.mp4](https://fedora.taila85941.ts.net:8912/daemon-stream.mp4).

This is native Slint verification with controlled stream timing and injected
window events, rather than a production filesystem or physical-input benchmark.
Queued UI batches are bounded, while the GUI still stores all displayed rows and
therefore uses O(M) model memory for M matches. The daemon must support streaming;
older daemons display an instruction to restart or rebuild `toged`.


## Compact Slint result storage — 2026-09-27

The GUI drops unused wire metadata, stores one boxed path per ordinary row, and
keeps exceptional wire display labels when they differ from the path. The
formatted-cell cache is bounded to 256 entries. Rename and trash mutate compact
rows in place instead of cloning the full result set. No match limit was added.

- All 28 Slint unit/IPC tests passed serially. New regressions exercise cache
  eviction/re-rendering, metadata invalidation, Unicode paths and custom labels.
  Sorting, selection, rename descendants and directory removal still pass.
- Strict Clippy, development and release client builds, workspace formatting,
  and `git diff --check` passed.
- Sequential native Winit/Skia development-client launches against the same
  existing daemon both displayed **1,996,760 / 1,996,760 matches** for an empty
  query. `/proc/<pid>/smaps_rollup` samples after completion and approximately
  five additional idle seconds reported:

  | UI memory | Before | After |
  | --- | ---: | ---: |
  | RSS | 987,276 KiB (964 MiB) | 440,136 KiB (430 MiB) |
  | PSS | 932,290 KiB (910 MiB) | 385,763 KiB (377 MiB) |

  RSS fell by 55.4%. These are single fresh-launch samples, excluding daemon
  memory, rather than repeated acceptance benchmarks. The original long-running
  client was separately observed at roughly 1.6 GiB RSS; its interaction history
  differs, so that is not the controlled before/after comparison. Both clients
  used the same configuration and compositor. Raw samples, screenshots and the
  measurement script are outside version control in
  `/tmp/toge-slint-memory-results/`.
- Native Slint visual verification used the shared `ui/main.slint`, changed
  result model, real worker and a controlled 4,096-row daemon stream fixture.
  WindowEvents exercised progressive loading, focus and keyboard selection,
  scrolling, cancellation, late selection restoration, sorting, empty results,
  errors and recovery. All assertions passed. Captured frames were inspected for
  clipping, visible selection, focus, loading status and final results.
- Recording: [memory-results.mp4](https://fedora.taila85941.ts.net:8914/memory-results.mp4)
  (tailnet only). Artifacts and fixture are in `/tmp/toge-slint-memory-visual/`.
  This is a native-window test with injected input and controlled stream timing;
  it is not a physical-input or production filesystem latency benchmark.

Result memory remains O(M); eliminating that would require daemon-backed paging
and sorting rather than retaining every match in the client.
