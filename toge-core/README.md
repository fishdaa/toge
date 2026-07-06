# toge-core

Shared library crate for the Toge workspace.

`toge-core` contains the reusable building blocks behind the daemon and CLI:

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
