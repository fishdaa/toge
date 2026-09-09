# Slint Rust GUI migration MVP

Status: experimental MVP implemented on branch `slint`. See
[`toge-slint/validation.md`](../toge-slint/validation.md) for completed checks and
remaining performance/desktop validation. The targets below remain acceptance goals.

## Objective

Deliver an experimental Linux `toge-slint` application that searches the existing
`toged` index with substantially lower UI RAM use and faster startup. Keep the
current Tauri release available until the prototype demonstrates those gains.
The MVP is a usable, manually launched search application; replacing the resident
tray/shortcut workflow is a later release gate.

## MVP scope

- One resizable window: focused search input, four-column results table
  (name, parent path, size, modified), compact status and error area.
- Existing query syntax passed unchanged to the daemon; 100 ms initial debounce,
  with Enter submitting immediately from the search input.
- Up/Down navigation, Enter to open from the results table, double-click to open,
  Ctrl+L to focus/select search text, and copy-path and open-parent actions.
  Copy shortcuts are scoped to results so text editing retains normal behavior.
- Ascending/descending column sorting, numeric size/date comparisons, selection
  tracked by full path, and automatic scrolling to the selected row.
- Maximum 10,000 returned rows, matching the current Tauri command default.
  Show “Showing N of M matches” when truncated. Sort only the loaded results;
  global sorting and paging are outside this MVP because the query request has
  no structured sort fields.
- Empty, searching, daemon-starting/indexing, ready, disconnected, and error states.
  Read existing configuration for size availability; offer Retry after failures.
- Start the daemon when absent, honor `TOGE_SOCKET` and existing XDG paths,
  and find `toged` beside the executable or on PATH.
- Closing the window exits the GUI. An independently running daemon stays alive.
- Standard Slint widgets and a restrained built-in style; use system fonts.

Defer tray, global shortcuts, autostart, multiple windows, Options editor,
Diagnostics window, delete/trash, custom keybinding editing, saved column widths,
visual redesign, and DEB/RPM/AppImage changes. Existing config can be edited through
the current application or config file. These omissions must be in the MVP README.

## Architecture and repository changes

```text
toge-slint/ui/main.slint
          | callbacks / model updates
toge-slint/src/controller.rs
          | bounded latest-query mailbox
toge-slint/src/client.rs (worker thread)
          | existing length-prefixed binary IPC over Unix socket
        toged -> toge-core
```

Add a `toge-slint` workspace crate with `Cargo.toml`, `build.rs`, `ui/main.slint`,
and Rust modules for controller, client, result model, formatting, and file actions.
Depend directly on `toge-core` for protocol/config types. Adapt the small existing
`toge-gui/src-tauri/src/ipc_client.rs` transport into the new client module, retaining
source provenance. Avoid linking `toge-gui-lib`, which would pull in Tauri. Defer
a shared client crate until the prototype earns a full migration.

Use `slint` and matching `slint-build` versions pinned in Cargo.lock. Begin with
explicit Winit backend and software renderer features, with default features
disabled and the required platform/compatibility features enabled. Verify exact
feature names against the chosen release. Benchmark an optional GPU renderer if
software scrolling misses the responsiveness target; software is a hypothesis,
not a promised performance win. Keep Qt/WebKit out of the MVP dependency graph.

Start with `StandardTableView`, which exposes selection and sort callbacks.
Use a Rust-backed model with one canonical result collection and a sorted index
mapping; format cells on demand where practical. Verify row virtualization and
allocation behavior with 10,000 rows before choosing a custom list implementation.
Do not retain previous result collections after replacement.

Run the Slint event loop on the main thread. Perform all socket I/O, daemon
readiness checks, and potentially blocking file actions on workers. Post results
through `Weak::upgrade_in_event_loop` or `invoke_from_event_loop`; create and
mutate Slint models only on the UI thread. Avoid a new async runtime for this MVP.

Increment query generation immediately on each input edit, including clearing.
Permit one active query and one replaceable pending query. Discard outdated
responses and errors even while the newest input is still debouncing. Bound
read/write waits, validate response IDs and frame lengths, and ensure shutdown
does not wait indefinitely for a stuck daemon. Retain the existing 10 MiB IPC
message limit. A daemon timeout must leave the window editable and Retry usable.

Launch the window before connecting to the daemon. Poll startup progress only
while needed and use infrequent status refresh once ready; do not create a
continuous repaint loop. Reconnect on subsequent searches or explicit Retry.
Surface action failures instead of inheriting the old shell's silent errors.

## Delivery sequence

| Step | Work | Completion evidence |
| --- | --- | --- |
| 1. Baseline and shell | Record optimized Tauri baseline; add crate, build integration, window, and fixture table | Cargo-only launch; responsive 10,000-row table on target desktop; recorded baseline |
| 2. Live search | Add transport, daemon startup/progress, bounded query scheduling, models and errors | Real queries match CLI/daemon responses; rapid typing cannot display stale results |
| 3. Usable MVP | Sorting, keyboard focus, selection, open/copy/parent actions, retry and clean exit | End-to-end manual search workflow and focused tests pass |
| 4. Evaluate and deliver | Measure release build, document limitations and dependencies, add developer commands | Performance report, runnable release binary, README and go/no-go decision |

Proposed developer entry points: `make slint` and `make slint-release`, backed by
a launcher following the existing development profile conventions. Build only
`toge-slint` and `toged`; no Node/Vite is needed for these commands. Preserve the
existing `make gui` commands. The launcher owns and cleans up only its own temporary
daemon/runtime directory and keeps development configuration isolated. Preserve
the existing explicit daemon capability setup for fanotify instead of adding
privilege changes to GUI startup.

## Performance acceptance

These are proposed targets, not measured results:

- At least 50% lower steady-state UI proportional set size (PSS) than release Tauri,
  both with an empty window and with the same 10,000-result fixture.
- Median launch-to-editable-window at least 30% faster than release Tauri and
  below 300 ms with warm filesystem caches on the designated test machine.
- No sustained idle repainting; target below 1% of one CPU core while ready/idle.
- No UI stalls above 100 ms during typing, result replacement, or scrolling through
  10,000 rows on the designated machine. Record how this was instrumented.

Compare release builds with identical daemon, index, query, row cap, window size,
display scaling, and desktop session. Include all UI-owned child processes in PSS;
report daemon memory separately and report combined application memory as well.
Record RSS and peak memory as supplementary metrics, not interchangeable with PSS.
Keep renderer/GPU memory limitations explicit in the measurements.

Run at least 20 warm launch trials and report median and p95. Separately report
first launch after login/reboot, daemon-already-ready launch, daemon-absent launch,
and time to first visible results. Mark unavailable measurements as unmeasured.
Do not conflate time spent indexing with UI startup. If targets fail, profile the
renderer, model allocations, and startup work before expanding migration scope.

## Validation and release boundary

Automated checks should cover stale responses/errors, debounce and bounded queue
behavior, disconnects/timeouts, malformed/oversized frames, numeric sorting, and
selection after sorting/replacement. Use a local fake Unix-socket server for
transport failures and deterministic fixtures for controller/model tests.

Run formatting, targeted Clippy/tests for the new crate, and a release build.
Smoke-test Wayland and X11, keyboard navigation/focus, double-click, clipboard,
Unicode/long paths, unavailable metadata, empty results, daemon restart, resizing,
and 10,000-row scrolling. Record unavailable desktop-session coverage explicitly.

Deliver an experimental executable plus instructions for its daemon/runtime
dependencies. Include Slint attribution using `AboutSlint` under its royalty-free
desktop license route; retain project source licensing and document third-party
notices. Check the exact dependency version's license terms before distribution.

After the MVP passes, plan tray/shortcut/activation behavior, settings migration,
autostart, diagnostics and package integration. Remove the old shell only after
those user workflows and release packages are validated. Passing MVP performance
targets alone does not establish full replacement readiness.

## Reference documentation

- [Slint Rust integration and threading](https://docs.slint.dev/latest/docs/rust/slint/)
- [StandardTableView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/standardtableview/)
- [Backends and renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)
- [Slint licensing and attribution](https://github.com/slint-ui/slint/blob/master/LICENSE.md)

Documentation reviewed on 2026-09-09; verify APIs against the pinned release during implementation.
