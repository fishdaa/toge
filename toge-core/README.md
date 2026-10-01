# toge-core

Shared library crate for the Toge workspace.

`toge-core` contains the reusable building blocks behind the daemon, CLI, and
Slint client:

- filesystem walking and exclusion rules
- in-memory indexing and persistence
- query parsing, matching, and result sorting
- config loading
- IPC request and response types
- ANSI highlighting helpers
- Linux watcher abstractions

## Modules

Public modules currently exposed by the crate:

- `config`
- `db`
- `highlight`
- `index`
- `ipc`
- `matcher`
- `opts`
- `query`
- `sort`
- `sys`
- `walker`

The crate also re-exports `Index` as `toge_core::Index`.

## Usage

Add the crate as a dependency from the workspace:

```toml
[dependencies]
toge-core = { path = "../toge-core" }
```

Example:

```rust
use toge_core::Index;

let index = Index::new();
assert_eq!(index.count(), 0);
```

## Development

Run the crate checks with:

```bash
cargo test -p toge-core
cargo run --release --example bench -p toge-core
cargo run --release --example profile -p toge-core -- insert
```

For the broader project overview, see the repository root [README.md](../README.md).

## Search filters

`parent:/path` (also `infolder:` or `nosubfolders:`) matches entries directly in
that folder, excluding descendants. Quote paths containing spaces, for example
`parent:"/home/user/My Files"`. `depth:` (also `parents:`) counts parent directory
components below `/`: `/file.txt` has depth 0 and `/home/user/file.txt` has depth 2.
It accepts a count, ranges such as `2..4`, and comparisons such as `>=2`.
`attrib:D` selects directories and `attrib:H` selects names beginning with a dot;
`attrib:DH` selects hidden directories. Extension filters ignore extension case
and exclude directories.

A leading `!` excludes names matching a term, for example `report !draft` or
`!*.tmp`. Quote the term to search for a literal `!`, as in `"!important"`.

`child:`, `empty:`, `diacritics:`, and readonly/system attribute filters return
explicit unsupported-filter errors. The current index does not store the
information needed to evaluate them reliably. Empty extension filters and
unterminated quoted values also return errors. Regex matching respects `case:`
and `nocase:`.

## Streaming queries

For a local index, `matcher::iter_query` yields IDs lazily in index order:

```rust
use toge_core::{Index, matcher::iter_query, query::Query};

let index = Index::new();
let query = Query::parse("ext:rs").unwrap();
for id in iter_query(&index, &query).take(100) {
    println!("{}", index.entries[id as usize].path);
}
```

This iterator scans the index and reads stored metadata. It does not sort or
apply result limits itself; use iterator adapters such as `skip` and `take`.
Dropping it cancels the scan. `QueryMatcher` also supports incremental matching
when the caller needs to refresh metadata between entries.

For a daemon connection, `ipc::stream_query` invokes a callback for each batch:

```rust,no_run
use std::os::unix::net::UnixStream;
use toge_core::ipc::{stream_query, OutputFormat, QueryRequest, StreamOrder, StreamQueryRequest};

let mut socket = UnixStream::connect("/path/to/toged.sock")?;
let request = StreamQueryRequest {
    query: QueryRequest {
        id: 1, raw: "ext:rs".into(), max_results: usize::MAX,
        offset: 0, format: OutputFormat::Default, highlight: false,
    },
    order: StreamOrder::Index,
};
let summary = stream_query(&mut socket, &request, |rows| {
    for row in rows { println!("{}", row.path); }
    Ok(())
})?;
println!("{} matches", summary.total_count);
# Ok::<(), std::io::Error>(())
```

Batches contain at most 128 rows, and client frames are limited to 4 MiB. A final
`StreamSummary` reports the full match count, total size, and returned row count.
EOF without that summary is an interrupted stream, not successful completion.
A callback error stops consumption; close/drop the socket to cancel the daemon.

`StreamOrder::Index` explicitly ignores sorting and never buffers or sorts
matches. Queries with a selective seed (an `ext:` filter or a substring of three
or more characters) first collect candidate IDs from the extension and trigram
posting lists via `matcher::candidate_ids`, so their extra memory grows with the
number of candidates. Seedless queries, like `matcher::iter_query`, use O(1)
extra memory with respect to the number of files: one batch, one encoded frame,
and matcher state.
`StreamOrder::Sorted` honors `Query.sort` and retains O(M) IDs for M matches,
while result-row serialization stays bounded to one batch.
Both modes honor the request's offset and limit, and scan all matches to produce
exact final totals. The pre-existing `Request::Query` protocol is unchanged.

Result sessions (`ipc::session`) keep a query's results on the daemon so a
client can fetch pages, resort, locate a path, and reconcile paths it changed on
disk without re-running the query. See the [toged README](../toged/README.md) for the request types
and limits.
