# Needle

Needle is a Linux-first file search project inspired by Everything and the ES CLI workflow.
It is organized as a Rust workspace with three parts:

- `needle-core`: shared indexing, query, and IPC logic
- `needled`: a background daemon that maintains the index
- `ndl`: a CLI client for issuing searches

## Status

The project is early and moving quickly. Expect frequent internal changes until the first stable release.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

## Versioning and Releases

Needle uses SemVer.

- Versions are declared once in the workspace root `Cargo.toml`.
- Git tags use the `vX.Y.Z` format.
- GitHub Actions runs CI on pushes and pull requests.
- Pushing a version tag builds release binaries and publishes a GitHub Release.

The detailed release checklist lives in `CONTRIBUTING.md`.
