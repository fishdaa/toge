# Toge Slint

Linux desktop client for the `toged` daemon. Uses Rust, Slint 1.17.1, Winit and
the software renderer. Release archives include `toge-slint` alongside the CLI
and daemon.

Download the archive for your Linux architecture from GitHub Releases, extract
it, and run `./toge-slint` from the extracted directory. Keep the bundled `toged`
next to it so the client uses a daemon with the matching session protocol.

```bash
make gui                  # isolated development daemon and settings
make gui-release          # same, optimized
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
toge-slint --hide         # hide the current window
```

With no GUI running, show, new-window, and toggle start one; hide has no effect.
The commands can also be bound in a compositor (for example niri `spawn`). Ctrl+N
opens a new window from inside the app. Each window has its own query, results, selection and
daemon session; table sort and column widths are shared. Toggle acts on the most
recently opened window and keeps its query and results while hidden. Closing a
window discards it; the GUI exits when no window, including hidden ones, remains, unless
the tray icon is registered.

The tray icon uses the freedesktop StatusNotifierItem protocol over D-Bus (KDE, most
Wayland panels such as Waybar or Noctalia, and GNOME with the AppIndicator extension).
Clicking it shows the current window; its menu offers Show Window, New Window, Toggle
Window, About Toge, Options… and Quit. While the icon is registered, closing the last window keeps
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
row for Open, Copy path, Open folder, Cut, Copy, Rename, Delete, and Delete
permanently. Ctrl+C copies
the file, Ctrl+X cuts it, and Ctrl+Shift+C copies its path. Paste files in your file
manager; Cut moves the source only when pasted. File clipboard formats support
GNOME and KDE file managers. Shift+arrow, Shift+Home/End and Shift+PageUp/PageDown
highlight a range. Actions resolve uncached rows from the daemon, including ranges
larger than the display cache; if the result order changes while loading the
selection, select the items again and retry. F2 or Rename edits the Name cell in place; Enter
saves and Escape cancels. Clicking elsewhere cancels an uncommitted rename.
Delete moves the selected items straight to Trash without confirmation. Restore
them from your file manager’s Trash if needed. Shift+Delete (or **Delete
permanently…**) skips the Trash: it asks for confirmation first, then removes the
file, or a folder with all its contents. Enter confirms and Escape cancels. A
symlink is removed itself, never its target. Rename never replaces an existing
destination, and a successful rename keeps the renamed item selected. After a
rename, trash, or permanent delete, the daemon re-reads the affected paths, so the list updates even
without the filesystem watcher. Click **toge** at the top left to open About.

Clicking a row moves keyboard focus to the table. Up/Down, PageUp/PageDown and
Home/End then move the selection, loading distant rows as needed.

The resizable right pane previews common images, SVG, PDF pages, and DOC,
DOCX, ODT, and RTF documents. PDFs render at the viewport's physical pixel
width, including display scaling, and re-render after resizing settles.
The visible page appears before adjacent pages are prefetched. Scroll through
pages; there are no zoom or Fit controls.

Text and code show line numbers and preserve indentation. Code starts with
wrapping off and supports horizontal scrolling; prose starts with wrapping on.
Use the Wrap button to switch, and Copy to copy the preview text. Click inside
the text pane for arrow keys, PageUp/PageDown, Home/End, and Ctrl+C. Selecting
another file resets both scroll directions. Optional `bat`/`batcat` supplies
syntax colors; without it the same view shows plain text. Source text is bounded
to 64 KiB; displayed lines are bounded to 1,024 columns, with a truncation label.

XLSX and ODS show bounded sheet data using Python 3's standard library: up to
eight sheets, 200 rows and 30 columns per sheet. The view shows stored values,
not recalculated formulas or original cell formatting. LibreOffice page rendering
is a fallback when data extraction fails.

Audio files show FFprobe metadata and embedded artwork, without playback.
Videos (including MP4, MKV, WebM, MOV, and AVI) show five sampled still frames in
a repeating cycle. The first frame appears before duration inspection and sample
seeking; it remains visible if sampling fails. The loop stops when selecting another file or hiding the
window. GIFs show one frame. Additional image codecs such as AVIF and HEIC work
when an installed renderer supports them.

Existing freedesktop thumbnails under `$XDG_CACHE_HOME/thumbnails` (normally
`~/.cache/thumbnails`) appear before a fresh render. The cache reader validates
`Thumb::URI`, `Thumb::MTime`, and optional source size, and tries the largest
available thumbnail first. It also handles otherwise unsupported file types
when a valid thumbnail exists. PDF cache images apply only to page one and must
be at least as wide as the viewport. Toge reads this cache without modifying it.

Previews use installed tools:

- Images: FFmpeg (`ffprobe` and `ffmpeg`), then ImageMagick (`magick` or
  `convert`) or GraphicsMagick (`gm`).
- SVG: `resvg` or `rsvg-convert` first, then the image backends.
- Video stills: FFmpeg decodes the first frame, then five samples once, within 640 × 640 pixels, and
  cycles those images. Audio is not decoded. Large video files have no image
  source-size limit; finite duration and bounded source dimensions are required.
- Audio metadata and cover art: `ffprobe` and `ffmpeg`.
- PDFs: Poppler (`pdftoppm`, with `pdfinfo` for page count), then MuPDF (`mutool`).
- Embedded office thumbnails: `gsf-office-thumbnailer` for DOC, DOCX, and ODT.
  Blank or missing thumbnails fall through to the next backend.
- Office page layout: LibreOffice (`soffice`, `libreoffice`, or `lowriter`)
  converts to a temporary PDF for a first-page preview.
- Office text when page rendering is unavailable: Python 3's standard library,
  then `unzip` with `xmllint` for DOCX/ODT; `catdoc` for DOC/RTF.
- Sheet data: Python 3's standard library.
- Code colors: optional `bat` or `batcat`; no Rust syntax-parser dependency.
  Fontconfig (`fc-match`) selects the installed monospace font.

Tool availability is detected once per process; restart after installing another
backend. Each backend is optional. Unsupported or unreadable files show a message;
selected-file metadata stays in the footer. Enter or double-click opens the
desktop's default viewer through `xdg-open`.

Conversion runs off the UI thread and cancels when selection changes. Source
limits are 32 MiB for raster images, 4 MiB for SVG, and 64 MiB for PDFs and office
documents. Images fit within 1,200 × 1,200 pixels. PDF bitmap width is bounded to
1,200 pixels. Image/PDF rendering times out after eight seconds, office conversion
after ten, and optional code highlighting after two.

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

Options… opens a separate window for keyboard shortcuts. App shortcuts can be
changed by clicking Record and pressing a key combination; they are saved to
`toge/shortcuts.conf` under the active XDG configuration directory. Press Escape
to cancel recording, or click Clear to disable a binding. Duplicate bindings in
the same scope are rejected. Systemwide shortcuts for showing, hiding, toggling,
and opening a new search window use the desktop Global Shortcuts portal when
available. The desktop can grant a different key than the requested one; its
shortcut settings are authoritative. On desktops without that portal, use the
compositor commands above. For niri development launches, bind the saved key
in `~/.config/niri/config.kdl` to the helper, which discovers the launcher's
temporary socket for the active profile:

```kdl
Ctrl+F10 { spawn "python3" "/path/to/toge/scripts/toggle-slint-dev.py"; }
```

Use the absolute checkout path in the binding. `TOGE_DEV_PROFILE` selects a
nondefault development profile. Niri handles the key even while Toge is hidden.
Autostart, diagnostics, and installer packages are not yet implemented; edit
daemon configuration in its configuration file.

Slint is used under its Royalty-free Desktop, Mobile, and Web Applications license;
the About dialog includes the `AboutSlint` attribution. See
[Slint's license notice](https://github.com/slint-ui/slint/blob/v1.17.1/LICENSE.md).
Toge source remains Apache-2.0.

Validation and benchmark results are tracked in [validation.md](validation.md).
