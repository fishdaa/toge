# toged

Background daemon for the Toge local file search workspace.

`toged` builds the filesystem index, serves search requests over a Unix domain
socket, persists index state, and keeps watch over indexed directories.

## Responsibilities

- load config and discover indexing roots
- build or restore the on-disk index
- answer query, status, save, and reindex requests
- maintain directory watches through the Linux watcher layer

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

The default socket and state files live under the Toge XDG state directory.

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

Without `--sort`, `--stream` explicitly uses index order (including when the raw
search string contains a `sort:` modifier). Use `--sort` to request sorted
streaming. Existing clients continue to use paginated, sorted queries.
Index-order streams scan lazily with bounded temporary memory; sorted streams
still collect matching IDs before emitting results.

Streams hold the index mutex to keep IDs, ordering, and totals consistent without
copying the index. Other queries and watcher updates wait until the stream ends.
Writes have a five-second stall timeout and a 60-second deadline checked during
scanning and writing; sorted preparation may run before the next deadline check.
Drop the connection to cancel. A transport error or deadline closes the stream;
clients must receive the final summary before treating output as complete.

CLI output is written/flushed one batch at a time. Streamed exports use a temporary
file and replace the destination only after successful completion.
