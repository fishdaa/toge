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
- streamed results for large result sets (`--stream`)

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
  --stream             Stream results in index order (--sort enables sorting)

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

If the daemon does not answer a status request, `toge` starts `toged`
(preferring a `toged` binary next to `toge`, then one on `PATH`) with the same
`--socket` path before querying.

The socket defaults to `$XDG_STATE_HOME/toge/toged.sock` (falling back to
`~/.local/state/toge/toged.sock`). Set `TOGE_SOCKET` to use another path.

### Additional options

The parser accepts more Everything-style flags than the short help lists. A
flag may be written with one or two leading dashes.

```text
Search:
  -a, -diacritics          Match diacritics
  -path <path>             Restrict results to a path
  -s                       Sort by path
  -sort <key>              Sort by key, for example name-asc or size-desc
  -sort-ascending, -sort-descending
  /ad, /a-d                Folders only, files only
  /oN /o-N /oS /o-S /oE /o-E /oD /o-D
                           Sort by name, size, extension, or date modified

Columns and output:
  -size, -dm, -dc, -ext    Show size, date modified, date created, extension
  -csv, -tsv, -txt, -efu   Output format
  -export-csv <file>       Write results to a file (also -export-tsv,
                           -export-txt, -export-efu)
  -no-header               Omit the header row
  -highlight               Highlight matches with ANSI colors
  -highlight-color <n>     ANSI color number for highlights (default 2)

Totals and exit status:
  -get-result-count        Print only the number of matches
  -get-total-size          Print only the total size of the matches
  -no-result-error         Exit with an error when nothing matches
  -hide-empty-search-results
                           Print nothing when there are no results
```

Any sort request (`-sort`, `-s`, `/o…`, or an inline `sort:` modifier) makes
`--stream` emit results in sorted order instead of index order.

## Relationship To Other Crates

- `toged` owns indexing and request handling
- `toge-core` provides shared parsing, IPC, and rendering helpers

For the overall project overview, see the repository root [README.md](../README.md).
