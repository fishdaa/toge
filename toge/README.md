# toge

Command-line client for querying the Toge search daemon.

`toge` connects to `toged` over a Unix domain socket, starts the daemon when
needed, sends search or maintenance requests, and prints the results in
terminal-friendly formats.

## Features

- daemon-backed local file search
- plain text, CSV, TSV, TXT, and EFU-style output modes
- optional ANSI highlighting for matches
- status, save, and reindex commands

## CLI

```text
toge [options] <search text>

Search options:
  -r, -regex <search>   Regex search
  -i, -case             Match case
  -w, -ww               Match whole word
  -p, -match-path       Match full path
  -o, -offset <n>       Start from result n
  -n, -max-results <n>  Max results

Info:
  -status               Daemon status
  -save-db              Force daemon to save index
  -reindex              Force daemon to rebuild index
  -h, -help             Show this help
  -v, -version          Show version
```

## Running

From the workspace:

```bash
cargo run -p toge -- --help
cargo run -p toge -- report
```

If the daemon socket is missing, `toge` will try to start `toged`
automatically before querying.

## Relationship To Other Crates

- `toged` owns indexing and request handling
- `toge-core` provides shared parsing, IPC, and rendering helpers

For the overall project overview, see the repository root [README.md](../README.md).
