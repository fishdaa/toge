# Contributing

Thanks for contributing to Needle.

## Workflow

1. Create a branch from `main`.
2. Make focused changes with tests when behavior changes.
3. Run the local quality checks before opening a pull request.
4. Open a pull request and wait for CI to pass before merging.

Recommended local checks:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

## Versioning

Needle follows Semantic Versioning:

- `MAJOR` for breaking public API or CLI behavior changes
- `MINOR` for backward-compatible features
- `PATCH` for backward-compatible fixes

The canonical version lives in the workspace root `Cargo.toml`.

## Release Process

1. Update `CHANGELOG.md`.
2. Bump the workspace version in `Cargo.toml`.
3. Verify local checks pass.
4. Commit the release change.
5. Create an annotated tag like `v0.1.1`.
6. Push the branch and tag to GitHub.

The release workflow validates that the tag matches the workspace version, runs checks, builds `ndl` and `needled`, and creates a GitHub Release with Linux artifacts.

## Branch Protection

Set these in GitHub repository settings:

- Require pull requests before merging to `main`
- Require the `CI` workflow to pass
- Restrict force pushes on protected branches

## Code Ownership

`CODEOWNERS` is configured so reviews default to the repository owner.
