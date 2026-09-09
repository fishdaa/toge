# Toge Slint MVP

Experimental Linux desktop client for the existing `toged` daemon. Uses Rust,
Slint 1.17.1, Winit and the software renderer. Tauri remains the supported release.

```bash
make slint                # isolated development daemon and settings
make slint-release       # same, optimized
```

The launcher keeps settings in `${XDG_CONFIG_HOME:-$HOME/.config}/toge-dev/slint/toge`
(with `TOGE_DEV_PROFILE` and `TOGE_DEV_CONFIG_ROOT` overrides). Index/socket state is
removed on exit, and only the launcher's daemon is stopped. For fanotify indexing,
apply the repository's existing capability setup explicitly after building:
`sudo ./scripts/setcap-toged.sh target/debug/toged` (use `target/release/toged` for
release). The launcher does not grant privileges itself.

To use your normal configuration and daemon:

```bash
cargo build --release -p toge-slint -p toged
./target/release/toge-slint
```

The app connects to `TOGE_SOCKET` or the usual XDG state socket, and starts `toged`
from its executable directory or PATH if absent. The GUI exits when closed; an
independent daemon continues running. Build requires Rust and Linux development
libraries for Winit/software rendering (including fontconfig and xkbcommon).
No Node, WebKit or Qt is needed for the Slint build. Runtime file actions use
`xdg-open`, and copying uses `wl-copy` or `xclip`/`xsel`.

Type the existing query syntax (`ext:pdf`, `folder:`, `path:src`, etc.). Search is
initially debounced by 100 ms; Enter in the search field submits immediately.
The initial empty query lists indexed entries. Use Up/Down in the table, Enter
or double-click to open, Ctrl+C in the table to copy the selected path, and Ctrl+L
to return to the search field. Buttons also expose open/copy/open-parent actions.
Column headers sort the loaded results. Size and modified time sort numerically.
Selection follows the full path across sorting and result replacement.

Results are capped at 10,000 and the status shows returned and total counts.
Sorting applies only to those returned rows. Missing indexed sizes display `—`.
Connection/indexing errors appear in the status area; Retry resubmits the current
query. Socket reads/writes have timeouts, and superseded results/errors are ignored.

This MVP does not implement tray/global shortcuts, autostart, settings editing,
diagnostics, destructive file actions, persistent column widths, multiple windows,
or installer packages. Edit configuration with the existing GUI or configuration
file. Keyboard bindings in this experimental shell are fixed.

Slint is used under its Royalty-free Desktop, Mobile, and Web Applications license;
the About dialog includes the `AboutSlint` attribution. See
[Slint's license notice](https://github.com/slint-ui/slint/blob/v1.17.1/LICENSE.md).
Toge source remains Apache-2.0.

Validation and benchmark results are tracked in [validation.md](validation.md).
The migration scope and acceptance targets are in
[the MVP plan](../docs/slint-mvp-plan.md).
