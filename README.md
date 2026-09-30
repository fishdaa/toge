# Toge

**Fast local file search for Linux, built as a daemon-backed Rust workspace.**

Toge is an open source project for indexing local files and querying them through a CLI-first workflow. The long-term aim is a search tool that feels immediate in the terminal, stays lightweight in memory, and scales cleanly from interactive use to shell scripts and automation.

## Why Toge

- Fast local search without depending on a GUI
- Daemon-backed queries for low-latency repeated lookups
- A CLI workflow designed for piping, scripting, and terminal use
- A modular Rust codebase with a shared core library

## Status

Toge is under active development. Tagged releases publish Linux binary archives
for the daemon, CLI, and Slint desktop client, along with beta and nightly
prerelease channels.

- Public interfaces may still change before `1.0`

## Workspace

Toge is split into four crates:

- `toge-core`: indexing, matching, sorting, config, and IPC primitives
- `toged`: background daemon that builds and serves the index
- `toge`: command-line client for querying the daemon
- `toge-slint`: Slint desktop client for the daemon

Repository layout:

```text
.
├── toge-core/     # shared library
├── toged/         # daemon binary sources
├── toge/          # CLI binary sources
├── toge-slint/    # desktop GUI sources
├── scripts/       # dev launcher, benchmarks, profiling, and release helpers
└── .github/       # CI, release, and repo automation
```

## Architecture

The intended runtime model is:

1. `toged` scans and watches configured filesystem roots
2. `toge-core` maintains the in-memory index and query engine
3. `toge` sends search requests over a Unix domain socket and prints results

Each crate's README documents its part of this flow: [toge-core](toge-core/README.md), [toged](toged/README.md), [toge](toge/README.md), and [toge-slint](toge-slint/README.md).

## Getting Started

### Requirements

- Linux
- Rust stable toolchain (edition 2024, so Rust 1.85 or newer)
- Font and keyboard development headers for the Slint client, for example
  `libfontconfig1-dev` and `libxkbcommon-dev` on Debian/Ubuntu
- `python3` and the libcap tools (`getcap`/`setcap`) for `make gui` and the
  launcher tests

The repository includes `rust-toolchain.toml` so the expected toolchain components are installed consistently for contributors.

### Build From Source

```bash
git clone https://github.com/fishdaa/needle.git
cd needle
cargo build --workspace
```

### Desktop GUI

Linux binary archives include `toge-slint`, `toge`, and `toged` for x86_64 and
ARM64. Extract the archive and run `./toge-slint`; keep the bundled daemon beside
it. The client uses Slint's software renderer and does not require Node or
WebKit. See [the Slint guide](toge-slint/README.md) for system dependencies,
keyboard controls, live-update setup, and current limitations. Release assets
also include SHA-256 checksum files.

`toged` needs Linux capabilities for filesystem-wide fanotify marks. Grant them
after extracting or rebuilding the daemon:

```bash
sudo ./scripts/setcap-toged.sh target/debug/toged
```

### GUI Development Profiles

`make gui` (alias `make slint`) keeps its settings separate from an installed
Toge instance. Its configuration persists under `~/.config/toge-dev/slint/toge`
and its index under `~/.local/state/toge-dev/slint/toge`; only the temporary
socket directory is removed when the development session ends.

Use `make gui-release` for performance testing. It uses the same isolated
development profile, but builds both `toge-slint` and `toged` with Rust
release optimizations.

Use a named profile when you need another independent set of development
settings:

```bash
TOGE_DEV_PROFILE=alternate make gui
```

Set `TOGE_DEV_CONFIG_ROOT` or `TOGE_DEV_STATE_ROOT` to override the parent
directory for all development profiles.

### Development Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
python3 -m unittest discover -s scripts/tests
```

### Benchmarks And Profiling

```bash
cargo run --release --example bench -p toge-core
cargo run --release --example profile -p toge-core -- insert
bash scripts/perf.sh run perf substring-miss substring-miss
bash scripts/perf.sh run perf substring-hit substring-hit
bash scripts/bench.sh run baseline
bash scripts/bench.sh compare 5
bash scripts/perf.sh compare perf substring-hit 5
```

The `bench` example prints quick timing summaries. The `profile` example keeps each hot path busy for longer so external profilers can capture useful samples. `substring`, `substring-miss`, and `substring-hit` default to more iterations than the other scenarios so the commands above produce denser captures without extra flags.

`bash scripts/perf.sh run <backend> ...` accepts the `perf`, `time`, and `heaptrack` backends and stores its output under `perf-results/<backend>/`, which is ignored by git. A `perf` run keeps both the binary capture and a text report:

```text
perf-results/perf/substring-miss.data
perf-results/perf/substring-miss.report.txt
```

Both helpers also keep a local timestamped history so you can compare the last `x` runs while iterating on performance work:

```text
bench-results/history/*.tsv
perf-results/history/<backend>/<label>/*.summary.tsv
```

## Project Goals

- Fast filename and path search on Linux
- Low-overhead indexing with room for optional metadata tiers
- Query behavior that works well both interactively and in scripts
- Clear separation between daemon, CLI, and shared core logic
- A contributor-friendly codebase with straightforward automation

## Release Model

Toge follows Semantic Versioning.

- The canonical version is declared in the workspace root `Cargo.toml`
- Stable Git tags use the `vX.Y.Z` format
- Beta prereleases use `vX.Y.Z-beta.N`
- A rolling `nightly` tag backs the nightly prerelease channel
- Pull requests must carry exactly one of `release:major`, `release:minor`, `release:patch`, or `release:none`
- Merging the automated release PR on `main` creates the matching stable tag and triggers release publishing
- GitHub Actions runs reusable checks on pull requests, `main`, `release/*`, and release tags
- Nightly prerelease artifacts are built from `main` on a daily schedule

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and release checklist.

## Roadmap

Near-term priorities:

- stabilize public interfaces ahead of `1.0`
- desktop integration for the Slint client: global shortcuts, autostart, and a settings UI
- installer packages for the Slint client

## Contributing

Contributions, bug reports, and design feedback are welcome.

If you want to help:

- read [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow
- review the crate READMEs and [Slint validation notes](toge-slint/validation.md) for project direction
- open an issue or pull request for focused, well-scoped changes

## Security

For security-sensitive reports, follow the guidance in [SECURITY.md](SECURITY.md).

## License

Toge is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the full text.
