# Toge Slint MVP

Experimental Linux desktop client for the existing `toged` daemon. Uses Rust,
Slint 1.17.1, Winit and the Skia renderer. Tauri remains the supported release.

```bash
make slint                # isolated development daemon and settings
make slint-release       # same, optimized
```

The launcher rebuilds both binaries and runs the executable paths reported by
Cargo, including custom target directories. It prints both paths before launching
and requires Python 3 to read Cargo's build messages.

The launcher keeps settings in `${XDG_CONFIG_HOME:-$HOME/.config}/toge-dev/slint/toge`
(with `TOGE_DEV_PROFILE` and `TOGE_DEV_CONFIG_ROOT` overrides). Index/socket state is
removed on exit, and only the launcher's daemon is stopped. After building, the
launcher checks the daemon's fanotify capabilities before starting it. If they
are missing, a native Slint dialog asks whether to enable live updates. Clicking
**Enable live updates** opens the system's Polkit authentication dialog; Toge does
not collect passwords. The launcher verifies access before starting the daemon.
Cancelling setup stops the launch without granting access. Cargo relinking removes
file capabilities, so rebuilt daemons may need approval again.

The approval flow requires libcap tools (`getcap`/`setcap`), `pkexec`, and a running
graphical Polkit authentication agent. An unchanged, already authorized binary
skips the dialog. Manual setup remains available via `scripts/setcap-toged.sh`.

This check verifies file capabilities; kernel policy, containers, or filesystem
restrictions can still prevent fanotify watches at runtime.

To use your normal configuration and daemon:

```bash
cargo build --release -p toge-slint -p toged
./target/release/toge-slint
```

The app connects to `TOGE_SOCKET` or the usual XDG state socket, and starts `toged`
from its executable directory or PATH if absent. The GUI exits when closed; an
independent daemon continues running. Build requires Rust and Linux development
libraries for Winit/Skia rendering (including fontconfig and xkbcommon).
No Node, WebKit or Qt is needed for the Slint build. Runtime file actions use
`xdg-open` and `gio trash`; the clipboard supports Wayland and X11 directly.

Type the existing query syntax (`ext:pdf`, `folder:`, `path:src`, etc.). Search is
initially debounced by 100 ms; Enter in the search field submits immediately.
The initial empty query lists indexed entries. Use Up/Down in the table, Enter
or double-click to open, and Ctrl+L to return to the search field. Right-click a
row for Open, Copy path, Open folder, Copy, Cut, Rename, and Delete. Ctrl+C copies
the file, Ctrl+X cuts it, and Ctrl+Shift+C copies its path. Paste files in your file
manager; Cut moves the source only when pasted. File clipboard formats support
GNOME and KDE file managers. F2 or Rename edits the Name cell in place; Enter
saves and Escape cancels. Clicking elsewhere cancels an uncommitted rename.
Delete moves the selected item straight to Trash without confirmation. Restore
it from your file manager’s Trash if needed. Rename never replaces an existing
destination, and a successful rename keeps the renamed item selected. Trash
actions update the displayed rows and clear selection. Click **toge** at the top
left to open About.

Column headers sort the loaded results. Size and modified time sort numerically.
Sorting clears the selection and returns the list to the top. The sort column
and direction are saved in `toge/slint-ui.toml` under the active XDG configuration
directory and restored, including the header arrow, on launch. Selection follows
the full path when search results are replaced.

The client uses the daemon's stream protocol in index order. Results appear in
batches of at most 128 during matching, rather than after the daemon collects and
sorts the full result set. Column headers still sort the received rows locally.
The status shows the received count while searching and exact totals only after
the completion summary. The GUI waits for each batch to be applied before reading
the next, keeping queued UI data bounded.

All matching rows are requested; there is no 10,000-row display cap. Each wire
frame is limited to 4 MiB; the full result set can span many frames. The GUI
stores one compact path and the displayed numeric metadata per received row, deriving ordinary Name/Path cells from that path. Formatted cells are cached
for at most 256 rows, so scrolling does not retain every visited row. Rename and
trash update the model in place. All matches remain available; result storage
still uses O(M) RAM for M displayed matches. Missing indexed sizes display `—`.

Editing a query or closing the GUI immediately shuts down the active query socket.
Stale batches and errors are ignored, and cancellation does not wait for another
batch to arrive. Streams hold the daemon's index lock, so cancellation also lets
new searches proceed once the daemon detects the closed socket. Use a daemon
built with stream support; older daemons display an instruction to restart or
rebuild `toged`.

Connection/indexing errors appear in the status area; Retry resubmits the current
query. Incomplete streams remain errors even if some rows arrived. Query responses
have a 30-second read timeout; status checks have a two-second timeout and retry
while the daemon is busy, within the readiness deadline.

This MVP does not implement tray/global shortcuts, autostart, settings editing,
diagnostics, persistent column widths,
or installer packages. Edit configuration with the existing GUI or configuration
file. Keyboard bindings in this experimental shell are fixed.

Slint is used under its Royalty-free Desktop, Mobile, and Web Applications license;
the About dialog includes the `AboutSlint` attribution. See
[Slint's license notice](https://github.com/slint-ui/slint/blob/v1.17.1/LICENSE.md).
Toge source remains Apache-2.0.

Validation and benchmark results are tracked in [validation.md](validation.md).
The migration scope and acceptance targets are in
[the MVP plan](../docs/slint-mvp-plan.md).
