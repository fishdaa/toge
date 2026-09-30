# Toge File Search

Search the [toge](https://github.com/fishdaa/needle) file index from the
Noctalia launcher. The toge daemon keeps the index up to date, so results show
up as you type, even across hundreds of thousands of files.

## Plugin

| Field | Value |
| --- | --- |
| ID | `fishdaa/toge` |
| Entries | Launcher provider: `launcher` |
| Launcher Prefix | `/toge` |

## Requirements

- A `toge` build on `PATH` that supports `--json`, `--no-wait` and `--`.
  These flags are available in the current checkout but not in stable `v0.2.1`.
  Build this checkout or use a nightly containing the CLI integration changes.
  With an older `toge`, the launcher shows
  "toge is too old for this plugin" instead of results.
- `toged`, the toge daemon, on `PATH` or installed next to `toge`. `toge`
  starts it on first use. Linux binary archives include both binaries and
  `toge-slint`; DEB and RPM packages are no longer produced.
- `xdg-open`, used to open results.
- `sleep`, used as a one-second timer while the index is still building.

## Usage

Open the launcher and type `/toge` followed by a query:

```
/toge report ext:pdf
/toge folder: src
/toge regex:^main\.rs$ sort:modified
```

Queries use toge's own syntax: `ext:`, `file:`, `folder:`, `path:`, `regex:`,
`case:`, `size:`, `dm:` and `sort:`. Activating a result opens it with
`xdg-open`. Folders open in the file manager.

With an empty query, the launcher shows the index status and these rows:

- **Rebuild index** runs `toge --reindex`.
- **Open Toge** starts the Toge GUI. This row appears only when the GUI is
  installed.

While the daemon is still loading or indexing, a progress row appears in place
of results. The plugin retries once a second, up to 120 attempts, while the
query remains current.

## Settings

| Setting | Type | Default | Description |
| --- | --- | --- | --- |
| `max_results` | `int` | `20` | Maximum rows listed per query (5–100). |
| `toge_command` | `string` | `toge` | Name or path of the toge CLI. |
| `gui_command` | `string` | `toge-slint` | Program started by the **Open Toge** row. |

## IPC

Rebuild the index, for example from a keybind:

```sh
noctalia msg plugin fishdaa/toge:launcher all reindex
```

## Notes

- For each query, the plugin runs `toge --json --no-wait -n <max> -- <query>`.
  The query goes to toge as a single argument, so no shell ever parses it.
- For an empty query, it runs `toge --status --json`.
- It also runs `toge --reindex`, `xdg-open <path>`, the GUI command, and
  `sleep 1` while the index is still building.
- The plugin writes no files and makes no network requests. The index and its
  settings belong to toge (`~/.config/toge/config.toml`).
- To develop against a local checkout, symlink it to
  `~/.local/share/noctalia/plugins/toge` and run
  `noctalia msg plugins enable fishdaa/toge`. Unit tests for `results.luau`
  run with any Lua 5.1+ interpreter: `luajit tests/results_test.lua`.
