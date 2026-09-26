# Toge Slint MVP

Experimental Linux desktop client for the existing `toged` daemon. Uses Rust,
Slint 1.17.1, Winit and the Skia renderer. Tauri remains the supported release.

```bash
make slint                # isolated development daemon and settings
make slint-release       # same, optimized
```

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

Results appear in batches of 128 while the IPC response transfers; the status
shows “Receiving…” until the final batch arrives. The daemon still completes
matching and sorting before sending. This uses the existing wire protocol and
works with older daemons. Editing the query discards stale batches.
All matching rows are requested; there is no 10,000-row display cap. The status
shows received and total counts. The existing 256 MB IPC frame limit still applies;
a larger response reports an error, so narrow the query in that case.
Column sorting applies to the rows received so far. Missing indexed sizes display `—`.
Connection/indexing errors appear in the status area; Retry resubmits the current
query. Query responses have a 30-second timeout; status checks have a two-second
timeout and retry while the daemon is busy, within the readiness deadline.
Superseded results/errors are ignored.

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
