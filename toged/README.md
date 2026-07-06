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
