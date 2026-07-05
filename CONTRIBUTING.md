# Contributing

Thanks for contributing to Needle.

## Workflow

1. Create a branch from `main`.
2. Make focused changes with tests when behavior changes.
3. Add exactly one release label to the pull request: `release:major`, `release:minor`, `release:patch`, or `release:none`.
4. Run the local quality checks before opening a pull request.
5. Open a pull request and wait for CI to pass before merging.

Branch naming conventions:

- `feature/<slug>` for normal work
- `release/x.y` for beta stabilization
- `hotfix/<slug>` for urgent patches from the latest stable tag

Recommended local checks:

```bash
cargo fmt --all -- --check
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

### Stable releases

1. Merge releasable pull requests into `main` with the correct release labels.
2. Let the `Release PR` workflow open or refresh the automated release PR.
3. Review and merge the release PR once the version bump and changelog look correct.
4. Create `release/x.y` from the merged release commit when you want a beta lane.
5. Push fixes to `release/x.y`; each push publishes a new `vX.Y.Z-beta.N` prerelease.
6. When the beta line is approved, run the `Promote Stable` workflow for that `release/x.y` branch.
7. The workflow creates the stable `vX.Y.Z` tag, which triggers the stable release publish flow.

### Nightly releases

- Nightly builds are published automatically from `main`.
- The nightly artifact version format is `X.Y.Z-nightly.YYYYMMDD+<sha>`.
- Nightlies are distributed as GitHub prerelease assets and are not published to the package registry.
- GitHub Releases requires a backing ref, so the workflow maintains a rolling `nightly` tag for that channel.

### Publishing requirements

- Set `CARGO_REGISTRY_TOKEN` in GitHub repository secrets before enabling registry publishing.
- Keep GitHub release permissions enabled for workflow-created tags and releases.

## Branch Protection

Set these in GitHub repository settings:

- Require pull requests before merging to `main`
- Require the `Checks` workflow to pass
- Require linear history on `main` and `release/*`
- Restrict force pushes on protected branches

## Code Ownership

`CODEOWNERS` is configured so reviews default to the repository owner.
