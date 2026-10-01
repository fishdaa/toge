# Slint validation history

This file records checks at the time of each change. Test counts, measurements,
renderers, limitations, and recordings belong to the named revision; older
entries are not descriptions of the current implementation. See the
[Slint README](README.md) for current behavior and requirements.

The client now uses daemon-held result sessions with a bounded page cache,
supports shortcut preferences and portal-backed global shortcuts, and previews
selected files. The preview Support popup was removed after the pane-sized
preview validation. Autostart, daemon configuration editing, and installer
packages remain unimplemented. This documentation alignment adds no new visual
test evidence.

## Initial MVP — 2026-09-09

Validated on Linux x86_64, Rust 1.97.1, Slint 1.17.1.

### Checks

- Release build: `cargo build --release -p toge-slint -p toged` passed.
- Seven unit/IPC tests passed with local Unix sockets enabled. Covers stale query
  invalidation before debounce, latest pending query replacement, numeric sorting,
  selection lookup, timestamp formatting, incorrect response IDs, malformed,
  oversized and truncated frames, and stalled-peer read timeout.
- `cargo clippy -p toge-slint --all-targets -- -D warnings` passed.
- Rust formatting, `git diff --check`, and launcher Bash syntax check passed.
- Normal dependency tree contains the software renderer and excludes Tauri,
  WebKit and Qt.

### Native window smoke test

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
- Physical-input keyboard, clipboard, double-click, About dialog and resize
  checks on both Wayland and X11. Later entries record native flows with injected
  window events and offscreen Slint checks; those do not establish physical-input
  coverage on both display systems.
- Native validation of shortcut preferences and desktop portal permission/key
  handling is not recorded here. Those features are implemented; autostart,
  daemon configuration editing and installer packages remain outside the
  current client. Release archives ship `toge-slint`.
- Tray icon behaviour (StatusNotifierItem registration, the menu's window and
  quit actions, and staying resident after the last window closes) has only
  unit coverage (`menu_offers_window_requests_about_and_quit`); it has no
  native desktop validation entry yet.

## Daemon stream integration — 2026-09-27

> Superseded by the daemon-held result sessions entry below: the client now
> opens a result session (`OpenSessionPreview`) and loads bounded pages instead
> of consuming a `StreamQuery`.

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

> Superseded by daemon-held result sessions below. The GUI now retains a bounded
> cache of fetched pages; the O(M) client memory and measurements in this entry
> describe the earlier implementation.

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

## Progressive session previews — 2026-09-27

Session opening previously waited for every matching ID and its final sort before
any rows could be shown. The Slint client now opts into `OpenSessionPreview`.
During the matching pass, the daemon sends the first match and growing previews
(up to 256 rows), then opens the normal sorted, paged session. Previews use index
order and cached metadata; the footer stays `Searching… (preview)` until the
final state arrives. Exact totals and final ordering still require completion.
Legacy `OpenSession` connections retain their original response sequence.

- Automated suites passed: 169 core, 27 Slint, 37 daemon, two workspace smoke
  checks and four daemon lifecycle tests. New tests prove preview consumption
  precedes completion, bounded storage, selection retention, cancellation, and
  real daemon preview frames followed by sorted page fetching. Strict Clippy,
  formatting and diff whitespace checks passed.
- Native Winit/Skia verification used the shared `ui/main.slint` and the actual
  client, model, worker and preferences modules against a controlled session
  fixture. It exercised keyboard search, visible rows while busy, final results,
  focus and selection, wheel scrolling and End to row 4,095, cancellation by a
  replacement query, empty results, errors and recovery. All assertions passed.
- Captured native window frames were inspected for clipping, visible selection,
  focus, scroll position, loading status and errors. The recording is available
  only on the tailnet at
  [search-preview.mp4](https://fedora.taila85941.ts.net:8916/search-preview.mp4).
  Fixture sources, frames and logs are outside version control under
  `/tmp/toge-preview-visual/`. Input was injected as Slint WindowEvents and daemon
  timing was controlled; this is not a production filesystem latency benchmark.
  Frames were captured at a nominal 10 Hz and encoded at 10 fps.

Both GUI and daemon must be rebuilt/restarted to enable the new request. An old
daemon displays an instruction to restart or rebuild `toged`.

## Daemon rebuild cost and blank handoff — 2026-09-27

The preview alone did not fix the multi-second daemon rebuild. Date sorting was
re-reading filesystem metadata for every match while holding the shared index
lock. Query opening, re-sorting and live rebuilding now use timestamps maintained
by indexing, reconciliation and watcher events, and hydrate only missing date
fields. Metadata-only watcher changes now bump the index revision so live date
and size orders refresh correctly.

The GUI also keeps its existing rows until the sorted first page arrives. Initial
opening and sorted/live rebuilds publish their first page together with the final
model state, preventing a temporary table of empty placeholders at the handoff.

- All automated suites passed: 170 core, 27 Slint, 38 daemon, two workspace smoke
  checks and four lifecycle tests. Strict Clippy passed. The timestamp regression
  covers cached date sorting, progressive opening and reordering after a
  metadata-only watcher update.
- Release-mode isolated Modified-sort comparison (median of three full queries):
  35,072 entries from the existing fixture snapshot took 65.8 ms before versus
  0.477 ms after. A synthetic scale test repeating those snapshot entries to
  2,000,000 entries took 4.025 s before versus 92.3 ms after. The scaled test
  uses repeated paths and warm filesystem metadata, not the production index.
  Sources and measurements are under `/tmp/toge-daemon-benchmark/`.
- The native shared Slint UI, real client and worker passed a recording with
  deliberate 1.5-second first-page delays and 500 ms live-refresh page delays.
  Assertions verify visible preview rows throughout the delayed handoff and no
  empty first page after final publication or live refresh. Keyboard/focus,
  scrolling to the last row, cancellation, empty results, errors and recovery
  also passed. Captured frames were inspected for clipping, focus, scroll
  position, loading status and final visible rows.
- Tailnet-only recording:
  [search-handoff.mp4](https://fedora.taila85941.ts.net:8916/search-handoff.mp4).
  Artifacts are in `/tmp/toge-preview-visual/`. This uses injected Slint
  WindowEvents and controlled daemon timing; production watcher throughput and
  absence of fanotify overflow have not been measured in this fixture.

## Multiple windows and toggle — 2026-09-27

The GUI is now single-instance per daemon socket. `toge-slint --new-window` and
`toge-slint --toggle` hand their request to the running process over
`toge-slint.sock`; Ctrl+N opens a window in-app. Each window owns its worker,
result session and status poller. The event loop now runs until the last window
closes, so a toggled-away window stays alive.

> Superseded: the instance socket is now `toge-slint-<hash>.sock` (see the
> instance isolation entry below), and with the tray icon registered, closing
> the last window no longer exits the process.

- All 35 Slint unit/IPC tests passed serially, including new handoff tests
  (argument mapping, request delivery, a second instance not stealing the live
  socket, and replacing a stale socket). Strict Clippy and formatting passed.
- Native verification ran the debug `toge-slint` and `toged` in a nested niri
  26.04 compositor (real Wayland tiling, only its output captured with grim)
  against a 130-file fixture. Keys were typed with wtype; windows were closed
  with niri's `close-window`, the same xdg close request as the title-bar
  button. 22 assertions passed: first launch opens one window; `--new-window`
  exits immediately and adds a second window in the same process; windows keep
  independent queries (`report` 60, `mkv` 40); Ctrl+N adds a third; closing it
  leaves two; `--toggle` hides and re-shows the same window with its query
  intact, twice; a plain launch opens nothing new; closing the last windows
  exits the process and removes the socket; `--toggle` with nothing running
  starts the GUI.
- Tailnet-only recording:
  [multi-window.mp4](https://fedora.taila85941.ts.net:8924/multi-window.mp4).
  Frames were captured at about 5 Hz, so the video runs near real time but is
  choppy. Artifacts are in the gitignored
  `visual-test-artifacts/multi-window-20260927/`.

wtype's `-M ctrl` did not apply Ctrl under niri, so Ctrl chords are sent as
Control_L presses. During probing, one injected sequence was read as Delete and
trashed a fixture file (restored before the recorded run). Raising an already
visible window depends on the compositor: Wayland does not let a client steal
focus without an activation token.


## Review fixes: instance isolation and large selection actions (2026-09-27)

- Instance sockets now use a daemon-filename hash within the daemon socket's
  directory. Missing parent directories are created with mode 0700 without
  changing existing directory permissions.
- Uncached range actions fetch paths in bounded batches on the session worker,
  independently of the display page cache. A changed result generation or
  superseding query cancels resolution before any file action executes.
- The range highlight is bounded to the visible viewport. Native verification
  exposed a software-renderer coordinate overflow for a 5,000-row highlight;
  the final run verified that this no longer crashes.
- All 42 `toge-slint` tests passed with `--test-threads=1`; Clippy passed with
  `--all-targets -- -D warnings`. An existing access-capability test failed once
  in a parallel run and passed on the serial rerun.
- Native shared `ui/main.slint` verification used the actual `toge-slint` binary
  and daemon on a nested headless Wayland compositor, with keyboard input from
  `wtype` and window capture from `grim`. It tested new-directory startup,
  search focus, End/Shift+Home scrolling and a 5,000-row selection, confirmation
  and cancellation, permanent deletion of only generated temporary fixture
  files, reconciliation to No matches, toggle handoff, and two daemon sockets
  in one directory with independent GUI processes.
- Captured frames were inspected for layout, clipping, focus, scroll position
  and visible errors. This is a native Slint window recording on a headless
  compositor; live fanotify updates and the user's desktop compositor were
  not verified. Artifacts are outside version control in
  `/tmp/toge-fix-visual/`.
- Tailnet-only recording:
  https://fedora.taila85941.ts.net:8938/recording.mp4

## Selected image previews — 2026-09-29

The Slint window now shows a right pane for selected PNG, JPEG, WebP, and GIF
images. One worker per window bounds file reads and decoded dimensions, scales
large images, and discards responses from older selections.

- All 46 Slint tests passed, including pointer drag and window resize coverage.
  Clippy passed with the existing workspace
  `uninlined_format_args` lint suppressed; the strict command stops in
  `toge-core/src/opts.rs:198`, outside this change. Formatting and diff checks
  passed.
- Native Wayland verification used the actual Slint client and daemon in an
  isolated nested niri compositor. The recording covers startup, large-image
  loading and display, keyboard focus and selection, portrait display,
  unsupported and damaged files, scrolling through 40 results, and Home
  returning to the first image. Captured frames were inspected for image fit,
  clipping, focus, scroll position, messages, and visible errors. The input was
  injected with `wtype`; live fanotify updates were outside this fixture.
- An offscreen Slint software-renderer recording of the same `ui/main.slint`
  covers pointer drag from 300 to 416 px. Captured frames show the results
  area narrowing and the image refitting without clipping. Native pointer
  injection was unavailable in the nested compositor.
- Tailnet-only recording:
  [image-preview.mp4](https://fedora.taila85941.ts.net:8952/image-preview.mp4)
  (native) and [preview-drag.mp4](https://fedora.taila85941.ts.net:8952/preview-drag.mp4)
  (offscreen Slint). Fixtures, frames, and logs are in
  `/tmp/toge-image-preview-visual-20260929/` and
  `/tmp/toge-image-preview-drag-20260929/`.

## Image preview memory — 2026-09-29

The preview worker decodes the source image before scaling it for the pane, so
large images create temporary full-resolution allocations. The selected Slint
image is replaced when selection changes; programmatically supplied images have
no Slint image-cache key.

An isolated native debug run with 36 distinct 2,000×1,200 JPEGs rose from
about 48 MiB RSS to 74 MiB after the first image, then stayed near 74 MiB
through the remaining selections. A second fixture with image dimensions rising
from 1,000×600 to 5,950×3,460 grew from 43 MiB to 111 MiB RSS; returning to
the first image brought it back to 47 MiB. This indicates a larger working set
for larger source images, not one retained image per selection.

The loader now releases compressed bytes before resizing, releases the decoded
source before copying pixels to Slint, and retains at most one completed result
while waiting for the UI thread. After this change, the increasing-size fixture
settled near 73 MiB RSS after the largest image. These are sampled RSS readings
from the isolated debug client, not measurements of short-lived allocation peaks.
All 46 Slint tests and Clippy with the existing workspace lint exception passed.
The final native preview flow was recorded again at the link above.

## More selected-file preview formats — 2026-09-29

The pane now previews BMP, ICO, TIFF, TGA and PNM in addition to the existing
PNG/JPEG/WebP/GIF formats. SVG is rasterized with resvg, and Poppler's
`pdftoppm` renders the first PDF page into a temporary PNG. UTF-8 text,
Markdown, logs, CSV, JSON, configuration, and common source files use a
scrollable text view. The worker still coalesces selection changes and keeps
only one pending result. PDF rendering is cancelled on a newer selection or
after eight seconds. File and output limits are documented in the README.

- `cargo test -p toge-slint --all-targets --offline`: 48 passed, including
  format classification, bounded text/binary handling, raster/SVG rendering,
  and divider resizing.
- `cargo clippy -p toge-slint --all-targets --offline -- -D warnings
  -A clippy::uninlined_format_args` and `git diff --check`: passed. The
  Clippy exception is for the existing `toge-core/src/opts.rs` lint.
- Native Wayland Slint recording covers startup and loading, BMP, SVG, PDF,
  Markdown, CSV, unsupported and damaged files, keyboard selection, scrolling
  the result list, and Home returning to the first image. Captured frames were
  inspected for image fit, text start position, clipping, focus, scroll
  position, and visible errors. The text view initially centered long content;
  setting its natural height and top alignment fixed that before the final run.
  Native pointer injection was unavailable, so this run uses keyboard input;
  divider dragging was separately checked with the offscreen Slint renderer in
  the earlier preview test.
- Tailnet-only recording:
  [formats-preview.mp4](https://fedora.taila85941.ts.net:8952/formats-preview.mp4).
  Fixture, screenshots, frames, and logs are in
  `/tmp/toge-formats-preview-visual-20260929/`.

## Office document previews — 2026-09-29

DOC, DOCX, ODT, and RTF results now use the existing preview pane. If
LibreOffice is installed, it converts the selected file to a temporary PDF,
then the existing Poppler path renders the first page. Without LibreOffice,
DOCX and ODT packages yield bounded text from their main XML entries, and
`catdoc` extracts bounded DOC and RTF text. Conversion cancels when selection
changes and times out after ten seconds. Temporary files are removed when the
worker finishes. The README lists limits and optional system tools.

- `cargo test -p toge-slint --all-targets --offline`: 49 passed, including a
  test that builds DOCX and ODT packages and checks paragraph extraction.
- `cargo clippy -p toge-slint --all-targets --offline -- -D warnings
  -A clippy::uninlined_format_args`: passed; the exception is for the
  existing workspace lint in `toge-core/src/opts.rs`.
- Native Wayland Slint recording with synthetic Word fixtures covers loading DOCX text, DOC text from
  `catdoc`, ODT text, PDF page rendering, RTF text, a damaged DOCX error,
  keyboard navigation, list scrolling, and Home. Frames were inspected for
  clipping, initial scroll position, focus, messages, and visible errors.
  A separately installed genuine DOC was also checked locally with `catdoc`;
  its contents are excluded from the shared recording. LibreOffice was absent, so page-layout conversion for office files could
  not be visually exercised here. Native pointer injection was unavailable;
  the earlier offscreen Slint test covers divider dragging.
- Tailnet-only recording:
  [documents-preview.mp4](https://fedora.taila85941.ts.net:8952/documents-preview.mp4).
  Fixture, screenshots, frames, and logs are in
  `/tmp/toge-documents-preview-synthetic-20260929/`.

## Installed-tool preview backends — 2026-09-29

The preview worker no longer depends on format-specific runtime Cargo crates.
It accepts bounded PAM/PPM output from installed tools and keeps the same
selection coalescing, cancellation, and single pending UI result. For images,
it tries FFmpeg, then ImageMagick (`magick`/`convert`) and GraphicsMagick (`gm`).
For PDFs, it tries Poppler then MuPDF. Office files use LibreOffice if present,
with text fallbacks through Python 3 or unzip/xmllint (DOCX/ODT), and catdoc
(DOC/RTF). Missing tools and failed decodes leave a visible message.

- `cargo test -p toge-slint --all-targets --locked --offline`: 48 passed.
  Tests cover format classification, bounded PAM/PPM parsing, system Python DOCX extraction, text limits,
  and the resizable divider. `cargo clippy -p toge-slint --all-targets --locked
  --offline -- -D warnings -A clippy::uninlined_format_args`,
  `cargo build -p toge-slint --locked --offline`, `cargo fmt --all -- --check`,
  and `git diff --check` passed. The Clippy exception is for the existing
  workspace lint in `toge-core/src/opts.rs`. Cargo.lock is unchanged.
- Native Wayland Slint recordings cover BMP, SVG, PDF, Markdown, CSV, DOCX,
  DOC, ODT and RTF, plus damaged/unsupported files, loading, keyboard focus,
  selection changes, result-list scrolling, and Home. Final frames were
  inspected for fit, initial text position, clipping, focus, and visible
  errors. Synthetic Word fixtures are used in the shared recording.
  A separate fixture with preview executables removed from the app PATH
  verified the visible missing-tool messages for images and PDFs.
  ImageMagick, GraphicsMagick, MuPDF, and LibreOffice were absent here, so
  those fallback executables could not be exercised in the native run.
  The unzip/xmllint text path was checked directly against DOCX/ODT fixtures.
  Native pointer injection was unavailable; the earlier offscreen Slint
  recording covers divider dragging.
- Tailnet-only recordings:
  [image and PDF flow](https://fedora.taila85941.ts.net:8952/system-tools-formats-final.mp4)
  [document flow](https://fedora.taila85941.ts.net:8952/system-tools-documents-final.mp4),
  and [missing-tool messages](https://fedora.taila85941.ts.net:8952/system-tools-missing.mp4).
  Frames and logs are in `/tmp/toge-system-tools-formats-final-20260929/`,
  `/tmp/toge-system-tools-documents-final-20260929/`, and
  `/tmp/toge-system-tools-missing-20260929/`.

## Embedded office thumbnails — 2026-09-29

The installed ONLYOFFICE Desktop Editors AppImage exposes a separate window,
so it cannot be placed inside Slint's Wayland preview pane. The optional
`gsf-office-thumbnailer` backend extracts a document's stored preview image
directly into the pane for DOC, DOCX, and ODT. If the image is missing or blank,
the existing LibreOffice page render or text fallback runs. The thumbnail is
a static stored image; it is not a live document editor.

- `cargo test -p toge-slint --all-targets --locked --offline`: 48 passed.
  `cargo build -p toge-slint --locked --offline`, `cargo fmt --all -- --check`,
  and `git diff --check` passed. Clippy was run with the existing
  `clippy::uninlined_format_args` workspace exception.
- Native Wayland Slint verification used the actual `toge-slint` binary and
  shared `ui/main.slint`. The recorded flow covers startup, thumbnail loading,
  an ODT's embedded first-page image, keyboard selection of a DOCX with a blank
  stored thumbnail and its text fallback, then a long text preview. Frames
  were inspected for image fit, clipping, focus, initial scroll position,
  and visible errors. The fixture uses a public ODT and synthetic DOCX/text.
  Native pointer injection was unavailable; divider dragging was verified
  separately in the earlier offscreen Slint recording. Capture was slower
  than the nominal frame rate, so the video plays faster than the interaction.
- Tailnet-only recording:
  [office-thumbnail-preview.mp4](https://fedora.taila85941.ts.net:8952/office-thumbnail-preview.mp4).
  Frames, fixture, and logs are in `/tmp/toge-office-thumbnail-visual-20260929/`.

## Remove preview Support button — 2026-09-30

Removed the preview header's Support button, its popup and unused diagnostic
summary. Updated the shared-UI fixture to stop opening the removed popup.

- `cargo test --workspace --all-targets --locked`: 345 tests passed on rerun.
  The first run hit a transient `Text file busy` error in an authentication test.
- `python3 -m unittest discover -s scripts/tests`: 22 tests passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Recorded the native shared Slint UI from startup through final assertions:
  PDF selection/loading, pane resize, page scrolling, code focus and keyboard
  input, horizontal scrolling, wrapping, text scroll reset, sheets, audio, SVG,
  desktop thumbnails, errors and rapid selection changes. Assertions passed.
- Inspected captured frames for header layout, clipping, focus, scroll position
  and visible errors; the Support button is absent before and after selection.
- [Recording](https://fedora.taila85941.ts.net:8960/remove-preview-support.mp4)
  is tailnet only. Artifacts are ignored under
  `visual-test-artifacts/remove-preview-support-20260930/`.
- Environment: native Slint/Winit software rendering in a headless Wayland Cage
  compositor with injected window input; physical desktop input was not tested.

## Pane-sized PDF, text and system previews — 2026-09-30

> The Support popup described and exercised in this entry was subsequently
> removed. See the removal entry above; the recorded popup checks describe
> the earlier UI.

PDFs now render one bitmap at the visible pane width multiplied by the window's
scale factor, bounded to 1,200 pixels. Zoom/Fit controls and duplicate fit renders
are removed. The worker publishes the current page before rendering neighbors,
retains at most three pages, and invalidates them when the source or width changes.
Text uses the installed monospace font, line numbers, optional bat colors, wrapping,
horizontal scrolling and keyboard focus. The pane also supports bounded XLSX/ODS
data, audio metadata/artwork, dedicated SVG tools and a backend support popup.

Desktop thumbnail reuse reads the freedesktop cache without modifying it, checks
the URI and modification time (plus source size when present), and rejects small
PDF thumbnails instead of stretching them. No PDF, SVG, office or syntax-parser
Cargo dependency was added. The only new lockfile package is the small MD5 helper
needed for standard thumbnail filenames; other direct dependencies already existed
in the dependency graph.

- `cargo test --workspace --all-targets --locked`: 345 tests passed.
- `python3 -m unittest discover -s scripts/tests`: 22 tests passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Final Slint tests and binary/example build passed before the native recording.
- Native shared-UI flow exercised PDF selection, pane resize, page scrolling,
  code focus/arrows/PageDown/Home, horizontal scroll, wrapping, text scroll reset,
  XLSX/ODS, audio with/without art, SVG, cached unsupported files, broken PDF,
  rapid selection cancellation and the Support popup. Assertions passed.
- PDF bitmap widths exactly matched measured physical viewports at 260 and 560
  pixels. The synthetic two-page PDF first appeared after 166 ms in this debug
  fixture; this is a fixture measurement, not a general performance benchmark.
- Inspected captured frames for text alignment, readability, clipping, scroll
  positions, loading/error messages and popup layout. The recording is 34 seconds:
  [preview-improvements.mp4](https://fedora.taila85941.ts.net:8958/preview-improvements.mp4)
  (tailnet only). Artifacts are ignored under
  `visual-test-artifacts/preview-improvements-20260930/`.
- Environment: native Slint/Winit software rendering in a headless Wayland Cage
  compositor, using injected window input. This verifies the shared Slint UI but
  does not exercise physical desktop input. Poppler, bat, Python and FFmpeg were
  available; SVG used FFmpeg's fallback. LibreOffice, dedicated SVG renderers and
  MuPDF were unavailable, so those optional backends were not visually verified.

[Hayro](https://github.com/laurenzv/hayro) was reviewed and deferred. Its Rust 1.92
minimum and Apache/MIT licensing are compatible, but upstream calls it experimental,
lists rendering gaps and says performance has not yet been a focus. Its renderer,
font and PDF image-codec dependency chain would bundle a substantial new subsystem.
It is worth reconsidering if self-contained rendering becomes a requirement;
there is no head-to-head Hayro performance or binary-size benchmark in this change.


## Video first-frame preview — 2026-09-30

Video previews now publish the actual first frame after bounded dimension
inspection, before probing duration or seeking to sampled positions. It remains
visible if later sampling fails. The pane labels the still while samples load.
Selection cancellation, source/output bounds, and the five-frame cycle remain in
place.

- `cargo test -p toge-slint --all-targets --locked --offline`: 61 application
  tests and 18 tests in each of the two examples passed. The video regression
  asserts that the first decoded frame is published before sampled frames,
  checks looping, and cancels immediately after the first frame.
- `cargo clippy -p toge-slint --all-targets --locked --offline -- -D warnings`,
  example build, formatting, and `git diff --check` passed.
- Native shared `ui/main.slint` recording covers selection/loading, a first
  frame visible during an intentionally delayed three-second duration probe,
  transition to sampled frames, a broken file, pane resizing, hide/show,
  a duration-probe failure retaining the still, and keyboard selection that
  cancels sampling and clears the image. All fixture assertions passed.
- Inspected captured frames for fit, clipping, selection/focus, error messages,
  and stale images. The recording uses a native Slint/Winit software-rendered
  window in a headless Wayland Cage compositor with injected input; it does not
  exercise physical desktop input. The synthetic probe delays test loading
  behavior and are not a performance benchmark. Artifacts are ignored under
  `visual-test-artifacts/video-first-frame-20260930/`.
- Tailnet-only recording:
  [video-first-frame.mp4](https://fedora.taila85941.ts.net:8959/video-first-frame.mp4).

## Search controls and filters — 2026-10-01

Implemented on `feature/search-controls-filters`, using the Everything reference
HTML under `legacy/`. The shared Slint search bar now provides case, whole-word,
path and regex controls, plus Everything, Files, Folders, Audio, Documents,
Pictures, Video and Archives presets. Typed queries and controls stay synchronized;
custom extension filters display Custom.

- `cargo test --workspace --all-targets --locked --offline -- --test-threads=1`
  passed. The subsequent empty-quoted-term regression also passed in the full
  186-test core library suite. `python3 -m unittest discover -s scripts/tests`
  passed all 22 launcher tests.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`,
  formatting and `git diff --check` passed.
- Parser/matcher regressions cover quoted parent paths, immediate-parent and
  depth filters, hidden filenames, unsupported-filter errors, grouped regex
  alternation and case matching, uppercase extensions, and restoring/removing
  entries from older extension buckets.
- Ordinary tests preceded the native visual run. The `search_controls_visual`
  example uses the production modules and shared `ui/main.slint` against an
  isolated real daemon indexing 1,028 fixture entries. All assertions passed:
  typing/loading, keyboard focus and checkbox toggles, all presets, typed-query
  synchronization, quoted parent and hidden filters, invalid filter/regex
  recovery, row navigation and scrolling, and compact layout at 560 pixels.
- Inspected recording frames for control layout, focus outlines, popup selection,
  query/error text, scrolling and compact layout. At narrow widths the controls
  use two rows and result columns remain horizontally scrollable.
- Recording, approximately 31 seconds:
  [search-controls.mp4](https://fedora.taila85941.ts.net:8964/search-controls.mp4)
  (tailnet only; Funnel is disabled). Artifacts are outside version control in
  `/tmp/toge-controls-verification-20261001/`.
- Environment: native Slint/Winit window in a nested Niri Wayland compositor,
  with injected keyboard and pointer events. Physical desktop input, tray and
  portal integration were not exercised. Empty fixture media/PDF files produce
  expected preview errors; this run verifies search interactions and layout.
