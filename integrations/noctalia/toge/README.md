# Toge File Search (Noctalia plugin)

Search the [toge](https://github.com/fishdaa/needle) file index from the
Noctalia v5 launcher. The daemon keeps the index, so results show up as you type
even across millions of files.

## Usage

Open the launcher and type `/toge` followed by a query:

```
/toge report ext:pdf
/toge folder: src
/toge regex:^main\.rs$ sort:dm
```

Queries use toge's own syntax: `ext:`, `file:`, `folder:`, `path:`, `regex:`,
`case:`, `size:`, `dm:`, `sort:`. Select a result to open it with `xdg-open`.
Folders open in the file manager.

With an empty query, the launcher shows the index status and these actions:

- **Rebuild index**: runs `toge --reindex`.
- **Open Toge**: starts the Toge GUI, if it is installed.

While the daemon is still loading or indexing, a progress row appears in place
of results. The provider retries once a second until the index is ready.

## Requirements

- Noctalia v5 with plugin API 24 or newer.
- A `toge` CLI that supports `--json`, `--no-wait` and `--`. `toged` must be on
  `PATH` or next to `toge`. The CLI starts the daemon on first use in its own
  process group, so launcher timeouts don't stop it.
- `xdg-open`, used to open results.
- `sleep`, used as a timer for the not-ready retry.

## Settings

| Key | Default | Meaning |
|---|---|---|
| `max_results` | 20 | Maximum rows per query (5–100) |
| `toge_command` | `toge` | Name or path of the toge CLI |
| `gui_command` | `toge-slint` | Program started by the **Open Toge** row |

## IPC

```sh
noctalia msg plugin fishdaa/toge:launcher all reindex
```

## Processes, files and network

- Runs `toge --json --no-wait -n <max> -- <query>` for each query. The query
  is passed as a single argument, so no shell is involved.
- Runs `toge --status --json` for an empty query.
- Runs `toge --reindex`, `xdg-open <path>`, the GUI command, and `sleep 1`
  while it waits for the index.
- Writes no files and makes no network requests.

## Development

```sh
ln -s "$PWD" ~/.local/share/noctalia/plugins/toge
noctalia msg plugins enable fishdaa/toge
luajit tests/results_test.lua   # unit tests for results.luau (any Lua 5.1+)
```
