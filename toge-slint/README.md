# Toge Slint MVP

Experimental Linux desktop client for the existing `toged` daemon. Uses Rust,
Slint 1.17.1, Winit and the software renderer. Tauri remains the supported release.

```bash
make slint                # isolated development daemon and settings
make slint-release       # same, optimized
```

The launcher rebuilds both binaries and runs the executable paths reported by
Cargo, including custom target directories. It prints both paths before launching
and requires Python 3 to read Cargo's build messages.

The launcher keeps settings in `${XDG_CONFIG_HOME:-$HOME/.config}/toge-dev/slint/toge`
(with `TOGE_DEV_PROFILE` and `TOGE_DEV_CONFIG_ROOT` overrides). The saved index
persists in `${XDG_STATE_HOME:-$HOME/.local/state}/toge-dev/slint/toge/index.bin`;
`TOGE_DEV_STATE_ROOT` overrides the `toge-dev` root. Debug and release launches
share the index within each profile. Cached results are loaded on subsequent
launches while filesystem changes are reconciled in the background. Only the
temporary socket directory is removed on exit, and only the launcher's daemon
is stopped. One launch can use each profile at a time; use a different
`TOGE_DEV_PROFILE` for a concurrent session. The launcher requires `flock`.
After building, the launcher checks the daemon's fanotify capabilities before
starting it. If they
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
from its executable directory or PATH if absent. An independent daemon keeps
running after the GUI exits.

One GUI process runs per daemon socket. It listens on `toge-slint-<socket-id>.sock` beside the
daemon socket, and later launches hand their request to it and exit:

```bash
toge-slint                # show the current window
toge-slint --new-window   # open another search window
toge-slint --toggle       # hide the current window, or show it again
```

With no GUI running, each form starts one. Bind these commands to compositor
shortcuts (for example niri `spawn`) in place of global hotkeys. Ctrl+N opens a new
window from inside the app. Each window has its own query, results, selection and
daemon session; table sort and column widths are shared. Toggle acts on the most
recently opened window and keeps its query and results while hidden. Closing a
window discards it; the GUI exits when no window, including hidden ones, remains, unless
the tray icon is registered.

The tray icon uses the freedesktop StatusNotifierItem protocol over D-Bus (KDE, most
Wayland panels such as Waybar or Noctalia, and GNOME with the AppIndicator extension).
Clicking it shows the current window; its menu offers Show Window, New Window, Toggle
Window, About Toge and Quit. While the icon is registered, closing the last window keeps
the GUI running in the tray, and Quit exits. Without a StatusNotifierItem host, or if the
host goes away while no window is open, the GUI exits as before.

Build requires Rust and Linux development
libraries for Winit rendering (including fontconfig and xkbcommon).
No Node, WebKit or Qt is needed for the Slint build. Runtime file actions use
`xdg-open` and `gio trash`; the clipboard supports Wayland and X11 directly.

Type the existing query syntax (`ext:pdf`, `folder:`, `path:src`, etc.). Search is
initially debounced by 100 ms; Enter in the search field submits immediately.
The initial empty query lists indexed entries. Use Up/Down in the table, Enter
or double-click to open, and Ctrl+L to return to the search field. Right-click a
row for Open, Copy path, Open folder, Copy, Cut, Rename, Delete, and Delete
permanently. Ctrl+C copies
the file, Ctrl+X cuts it, and Ctrl+Shift+C copies its path. Paste files in your file
manager; Cut moves the source only when pasted. File clipboard formats support
GNOME and KDE file managers. Shift+arrow, Shift+Home/End and Shift+PageUp/PageDown
highlight a range. Actions resolve uncached rows from the daemon, including ranges
larger than the display cache; if the result order changes while loading the
selection, select the items again and retry. F2 or Rename edits the Name cell in place; Enter
saves and Escape cancels. Clicking elsewhere cancels an uncommitted rename.
Delete moves the selected item straight to Trash without confirmation. Restore
it from your file manager’s Trash if needed. Shift+Delete (or **Delete
permanently…**) skips the Trash: it asks for confirmation first, then removes the
file, or a folder with all its contents. Enter confirms and Escape cancels. A
symlink is removed itself, never its target. Rename never replaces an existing
destination, and a successful rename keeps the renamed item selected. After a
rename or trash, the daemon re-reads the affected paths, so the list updates even
without the filesystem watcher. Click **toge** at the top left to open About.

Clicking a row moves keyboard focus to the table. Up/Down, PageUp/PageDown and
Home/End then move the selection, loading distant rows as needed.

Column headers sort in the daemon, using cached whole-index name and path
orders, so sorting a million matches takes milliseconds. Size and modified time sort numerically.
Sorting clears the selection and returns the list to the top. The sort column
and direction are saved in `toge/slint-ui.toml` under the active XDG configuration
directory and restored, including the header arrow, on launch. Resized Name,
Path and Size column widths are saved to the same file once a drag settles and
restored on launch; Modified fills the remaining width. Selection follows
the full path when search results are replaced or refreshed.

Results live in the daemon. Each search opens a *result session* on its own
connection: the daemon keeps the matching index IDs (4 bytes per match) and the
GUI fetches only the 256-row pages the table displays, plus the adjacent page.
At most 16 pages are kept, so GUI memory does not grow with the match count.
Rows not yet fetched render blank until their page arrives. The exact total is
shown as soon as the query finishes. There is no display cap.

While a session is open the GUI asks the daemon once a second whether the index
changed. Additions are picked up at most once a second, or less often for
expensive queries. Removals always rebuild the results before any more rows are
served, because they renumber index IDs.

Editing a query or closing the GUI immediately shuts down the active session
socket, which discards the daemon's copy of the results. Stale responses are
ignored. The daemon holds its index lock only while answering one request, never
while writing to the socket. Use a daemon built with session support; older
daemons display an instruction to restart or rebuild `toged`.

Connection/indexing errors appear in the status area; Retry resubmits the current
query. Session requests have a 30-second read timeout; status checks have a
two-second timeout and retry while the daemon is busy, within the readiness
deadline.

This MVP does not implement built-in global shortcuts, autostart, settings editing,
diagnostics, or installer packages. Edit configuration with the existing GUI or configuration
file. Keyboard bindings in this experimental shell are fixed.

Slint is used under its Royalty-free Desktop, Mobile, and Web Applications license;
the About dialog includes the `AboutSlint` attribution. See
[Slint's license notice](https://github.com/slint-ui/slint/blob/v1.17.1/LICENSE.md).
Toge source remains Apache-2.0.

Validation and benchmark results are tracked in [validation.md](validation.md).
The migration scope and acceptance targets are in
[the MVP plan](../docs/slint-mvp-plan.md).
