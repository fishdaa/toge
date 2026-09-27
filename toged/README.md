# toged

Background daemon for the Toge local file search workspace.

`toged` builds the filesystem index, serves search requests over a Unix domain
socket, persists index state, and keeps watch over indexed directories.

## Responsibilities

- load config and discover indexing roots
- build or restore the on-disk index
- answer query, streaming query, result session, status, save, reindex, and
  shutdown requests
- watch indexed filesystems through fanotify, skipping its own state and config
  directories and configured excludes

## CLI

```text
toged [options]

Options:
  --socket <path>     Unix domain socket path
  --config <path>     Config file path
  --state-dir <path>  State directory (for index.bin)
  --clean             Delete old index before starting
  -h, --help          Show this help
  -v, --version       Show version
```

## Running

Start the daemon directly from the workspace:

```bash
cargo run -p toged -- --help
cargo run -p toged
```

Default locations:

- config: `$XDG_CONFIG_HOME/toge/config.toml` (fallback
  `~/.config/toge/config.toml`); a missing or unreadable config falls back to
  built-in defaults
- state directory: `$XDG_STATE_HOME/toge` (fallback `~/.local/state/toge`),
  holding `index.bin` and the `toged.sock` socket

The state directory is created owner-only, and the daemon rejects socket peers
whose UID differs from its own.

### Capabilities

Filesystem-wide fanotify marks need `cap_sys_admin` and `cap_dac_read_search`.
Rebuilding the binary with Cargo drops file capabilities, so grant them again
after each build:

```bash
sudo ./scripts/setcap-toged.sh target/debug/toged
```

`toge -status` prints a hint when the watcher is not healthy.

## Relationship To Other Crates

- `toge-core` provides indexing, query, IPC, and watcher primitives
- `toge` acts as the user-facing command-line client

For the overall architecture, see the repository root [README.md](../README.md).

## Streaming results

The daemon accepts `Request::StreamQuery` on one connection and emits bounded
`StreamEvent::Rows` batches followed by `StreamEvent::Done` with final totals.
Parse/readiness failures produce `StreamEvent::Error`. Use
`toge_core::ipc::stream_query` to consume the frames without collecting them.

The CLI can use this directly:

```bash
toge --stream ext:rs
toge --stream --sort name-asc ext:rs
toge --stream --export-csv results.csv ext:rs
toge --stream --get-result-count ext:rs
```

`--stream` uses index order unless a sort is requested (`--sort`, `-s`, `/o…`,
or an inline `sort:` modifier), in which case it streams sorted results.
Index-order streams never buffer or sort matches, but selective queries (`ext:`
filters or substrings of three or more characters) first collect candidate IDs
from the posting lists. Sorted streams collect all matching IDs before emitting
results.

## Result sessions

`Request::OpenSession` and `Request::OpenSessionPreview` keep a query's results
on the daemon for the lifetime of the connection. The client then sends
`toge_core::ipc::session::SessionRequest` messages: `Fetch` a page of rows,
`Resort`, `Locate` a path, `Sync` after index changes, or `Reconcile` paths the
client changed on disk (for example after a rename, trash, or permanent delete).
Every response carries the session generation and totals. Fetches return at most
1,024 rows, previews at most 256, and one reconcile at most 256 paths. The Slint
client uses sessions for paging and sorting.

Streams hold the index mutex to keep IDs, ordering, and totals consistent without
copying the index. Other queries and watcher updates wait until the stream ends.
Writes have a five-second stall timeout and a 60-second deadline checked during
scanning and writing; sorted preparation may run before the next deadline check.
Drop the connection to cancel. A transport error or deadline closes the stream;
clients must receive the final summary before treating output as complete.

CLI output is written/flushed one batch at a time. Streamed exports use a temporary
file and replace the destination only after successful completion.
