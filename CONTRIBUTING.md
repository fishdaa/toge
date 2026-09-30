# Contributing

Thanks for contributing to Toge.

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
python3 -m unittest discover -s scripts/tests
```

The Python suite includes `scripts/tests/test_docs.py`, which fails when the
`toge`/`toged` README CLI blocks drift from each binary's `usage()` text, when a
parsed CLI flag is missing from `toge/README.md`, when `release-notes.md` and the
current `CHANGELOG.md` section disagree, or when a Markdown link points at a
missing or gitignored file. Update the docs in the same change as the code.

The guides describe the current checkout. Document new behavior under
`CHANGELOG.md`'s Unreleased section; keep `release-notes.md` aligned with the
current released version until preparing the next release. Preserve dated
Slint validation results as history, and mark superseded behavior without
claiming new visual coverage from a documentation update.

For UI changes, follow [AGENTS.md](AGENTS.md): run automated tests first, then
record and inspect the affected visual flow. Slint verification must exercise
`toge-slint/ui/main.slint`, using a native window where possible or the offscreen
Slint software renderer. Keep artifacts outside version control and share a
tailnet-only recording URL when Tailscale is available.

## Versioning

Toge follows Semantic Versioning:

- `MAJOR` for breaking public API or CLI behavior changes
- `MINOR` for backward-compatible features
- `PATCH` for backward-compatible fixes

The canonical version lives in the workspace root `Cargo.toml`.

## Release Process

### Stable releases

1. Merge releasable pull requests into `main` with the correct release labels.
2. Let the `Release PR` workflow open or refresh the automated release PR.
3. Review and merge the release PR once the version bump and changelog look correct.
4. Merging the release PR to `main` creates the stable `vX.Y.Z` tag automatically, which triggers artifact packaging and stable publishing.
5. Create `release/x.y` from the stable release commit when you want a beta lane.
6. Push fixes to `release/x.y`; each push publishes a new `vX.Y.Z-beta.N` prerelease.
7. When the beta line is approved, run the `Promote Stable` workflow for that `release/x.y` branch if you need to publish directly from the beta branch.

### Nightly releases

- Nightly builds are published from `main` on a daily schedule and can also be started manually.
- The nightly artifact version format is `X.Y.Z-nightly.YYYYMMDD.<sha>`.
- Nightlies are distributed as GitHub prerelease assets and are not published to the package registry.
- GitHub Releases requires a backing ref, so the workflow maintains a rolling `nightly` tag for that channel.

### Publishing requirements

- Set `CARGO_REGISTRY_TOKEN` in GitHub repository secrets before enabling registry publishing.
- Keep GitHub release permissions enabled for workflow-created tags and releases.
- Enable `Read and write permissions` and `Allow GitHub Actions to create and approve pull requests` in repository Actions settings.

## Branch Protection

Set these in GitHub repository settings:

- Require pull requests before merging to `main`
- Require the `Checks` workflow to pass
- Require linear history on `main` and `release/*`
- Restrict force pushes on protected branches

## Code Ownership

`CODEOWNERS` is configured so reviews default to the repository owner.
