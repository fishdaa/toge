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


## Daemon-held result sessions — 2026-09-27

The GUI no longer copies result rows over IPC. The daemon keeps each query's
matching IDs in a per-connection session. The GUI fetches 256-row pages on demand
and asks the daemon to re-sort, locate paths, and re-read renamed or trashed
paths. Retained IDs are tied to an index epoch that changes on every removal or
reindex, so the daemon never serves a row whose ID has been renumbered.

- Unit tests: 159 core (session wire round trips, malformed frames, client
  response checks), 32 daemon (ranges, resort, locate, removal and reindex
  invalidation, paced live sync, reconcile limited to configured roots, served
  session lifecycle) and 26 Slint (placeholder/page loading, stale generations,
  bounded page cache, fast-scroll fetch coalescing, sort-key mapping,
  cancellation). Clippy with `-D warnings` and rustfmt are clean. The existing
  temporary-script `access` test failed once in a parallel run and passed on
  every rerun.
- Native Niri/Wayland verification of the real `toge-slint` binary and shared
  `ui/main.slint`, against an isolated `toged` indexing 35,000 files (30,000
  sparse `.mkv` files with distinct sizes and times) on btrfs. Injected Slint
  WindowEvents typed `.mkv` (30,000 matches in ~0.45 s), wheel-scrolled about
  1,200 rows deep, clicked and arrow-keyed through deep rows, sorted by size
  both ways, renamed the largest file inline (it stays selected at row 0),
  showed the empty state, and searched for the renamed file. All 13 assertions
  passed. Frames were inspected for placeholders, clipping, focus, selection,
  sort arrows and status text.
- Recording: [daemon-sessions.mp4](https://fedora.taila85941.ts.net:8913/daemon-sessions.mp4)
  (tailnet only). The fixture, log and contact sheet are in the gitignored
  `visual-test-artifacts/daemon-sessions-20260927/`. Frames were captured with
  `Window::take_snapshot` about 8 times a second, so the video runs faster than
  real time. The input was injected rather than physical, and the watcher had no
  fanotify capability, so live watcher updates were not part of the visual run.
- Follow-up: clicking a row now moves keyboard focus to the table, and
  Home/End/PageUp/PageDown move the selection (the table widget handles only
  Up/Down). End jumps to row 29,999, loads its page, and pins the viewport to the
  bottom so the final row is not clipped. The re-run passed all 17 assertions
  with no Tab presses needed, and the recording at the same URL was replaced.

## Session query latency — 2026-09-27

Opening a session sorted every match by name before the first page (about
360 ms for 820k results), and superseded keystrokes queued on the index lock.
The daemon now keeps whole-index name and path orders (`OrderCache`), so a
query picks its matches in one pass over the cached order or sorts them by rank.
Appended entries are merged in about 2 ms; removals and reindexes rebuild it
(about 110 ms once). Superseded opens are skipped when the client has already
hung up, and the bulk zero-size `stat` pass runs only for size-sorted sessions,
since fetched rows refresh their own metadata.

Release daemon on a copy of the real 820k-entry index, open plus first 256-row
page, median of 5:

| Query | Matches | Before (match + per-query sort, timed separately) | After (end to end) |
| --- | ---: | ---: | ---: |
| (empty) | 816,750 | ~370 ms | 10.6 ms |
| `.` | 688,642 | ~290 ms | 28.8 ms |
| `s` | 494,304 | ~210 ms | 26.0 ms |
| `.mkv` | 31,113 | ~3 ms | 2.0 ms |
| `ext:mkv` | 31,113 | ~2 ms | 0.6 ms |

Typing `.` `.m` `.mk` then `.mkv` in a burst: final results ready 30 ms after
the first keystroke. Remaining time for broad queries is the matcher's scan.
Cached orders produce exactly the same order as `sort_ids`, which the tests
check across result sizes, both directions, ties, and merged insertions. The
native visual run passed all 17 assertions again.
