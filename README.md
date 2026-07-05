# Needle

**Fast local file search for Linux, built as a daemon-backed Rust workspace.**

Needle is an open source project for indexing local files and querying them through a CLI-first workflow. The long-term aim is a search tool that feels immediate in the terminal, stays lightweight in memory, and scales cleanly from interactive use to shell scripts and automation.

## Why Needle

- Fast local search without depending on a GUI
- Daemon-backed queries for low-latency repeated lookups
- A CLI workflow designed for piping, scripting, and terminal use
- A modular Rust codebase with a shared core library

## Status

Needle is pre-release software and still under active development.

- The workspace structure, architecture, and automation are in place
- Core modules and tests are being built out in the open
- Public interfaces may still change before `1.0`

If you are evaluating the project today, think of it as an early open source build rather than a finished end-user release.

## Workspace

Needle is split into three crates:

- `needle-core`: indexing, matching, sorting, config, and IPC primitives
- `needled`: background daemon that builds and serves the index
- `ndl`: command-line client for querying the daemon

Repository layout:

```text
.
├── needle-core/   # shared library
├── needled/       # daemon binary
├── ndl/           # CLI binary
├── needle-docs/   # architecture and design notes
└── .github/       # CI, release, and repo automation
```

## Architecture

The intended runtime model is:

1. `needled` scans and watches configured filesystem roots
2. `needle-core` maintains the in-memory index and query engine
3. `ndl` sends search requests over a Unix domain socket and prints results

The broader design and indexing strategy are documented in [needle-docs/architecture.md](needle-docs/architecture.md).

## Getting Started

### Requirements

- Linux
- Rust stable toolchain

The repository includes `rust-toolchain.toml` so the expected toolchain components are installed consistently for contributors.

### Build From Source

```bash
git clone https://github.com/fishdaa/needle.git
cd needle
cargo build --workspace
```

### Development Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Note: parts of the implementation are still stubbed, so some tests currently fail until those modules are completed.

### Benchmarks And Profiling

```bash
cargo run --release --example bench -p needle-core
cargo run --release --example profile -p needle-core -- insert
bash scripts/perf.sh run substring-miss substring-miss
bash scripts/perf.sh run substring-hit substring-hit
bash scripts/bench.sh run baseline
bash scripts/bench.sh compare 5
bash scripts/perf.sh compare substring-hit 5
```

The `bench` example prints quick timing summaries. The `profile` example keeps each hot path busy for longer so external profilers can capture useful samples. `substring`, `substring-miss`, and `substring-hit` default to more iterations than the other scenarios so the commands above produce denser captures without extra flags.

`bash scripts/perf.sh run ...` stores both the binary capture and a text report in `perf-results/`, which is ignored by git:

```text
perf-results/substring-miss.data
perf-results/substring-miss.report.txt
```

Both helpers also keep a local timestamped history so you can compare the last `x` runs while iterating on performance work:

```text
bench-results/history/*.tsv
perf-results/history/<label>/*.summary.tsv
```

The current perf takeaways and optimization notes live in `needle-docs/findings.md`.

## Project Goals

- Fast filename and path search on Linux
- Low-overhead indexing with room for optional metadata tiers
- Query behavior that works well both interactively and in scripts
- Clear separation between daemon, CLI, and shared core logic
- A contributor-friendly codebase with straightforward automation

## Release Model

Needle follows Semantic Versioning.

- The canonical version is declared in the workspace root `Cargo.toml`
- Stable Git tags use the `vX.Y.Z` format
- Beta prereleases use `vX.Y.Z-beta.N`
- A rolling `nightly` tag backs the nightly prerelease channel
- Pull requests must carry exactly one of `release:major`, `release:minor`, `release:patch`, or `release:none`
- Merging the automated release PR on `main` creates the matching stable tag and triggers release publishing
- GitHub Actions runs reusable checks on pull requests, `main`, `release/*`, and release tags
- `main` also publishes nightly prerelease artifacts automatically

See [CONTRIBUTING.md](CONTRIBUTING.md) for the contribution workflow and release checklist.

## Roadmap

Near-term priorities:

- complete the unfinished `needle-core` implementations
- bring the current test suite to green
- define the first usable daemon/client interaction flow
- stabilize basic indexing and search behavior
- publish the first pre-release binaries

## Contributing

Contributions, bug reports, and design feedback are welcome.

If you want to help:

- read [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow
- review [needle-docs/architecture.md](needle-docs/architecture.md) for project direction
- open an issue or pull request for focused, well-scoped changes

## Security

For security-sensitive reports, follow the guidance in [SECURITY.md](SECURITY.md).

## License

Needle is licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the full text.
