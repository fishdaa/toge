.PHONY: help build build-release test test-coverage watcher-test clippy fmt \
       gui gui-release gui-package \
       bench bench-compare perf perf-compare \
       set-version publish package verify-tag next-beta changelog

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'

# ── Build ──────────────────────────────────────────────

build: ## Build all crates (debug)
	cargo build

build-release: ## Build all crates (release)
	cargo build --release

# ── Test ───────────────────────────────────────────────

test: ## Run all Rust tests
	cargo test

watcher-test: ## Run watcher self-test coverage (Rust + GUI store test)
	cargo test -p toge-gui-lib
	cd toge-gui && npm test -- --run tests/stores/search.test.ts

test-coverage: ## Run Rust tests with coverage
	cargo llvm-cov --html

# ── Lint ───────────────────────────────────────────────

clippy: ## Run clippy on all crates
	cargo clippy --workspace --all-targets -- -D warnings

fmt: ## Format all code
	cargo fmt --all

# ── GUI ────────────────────────────────────────────────

gui: ## Run GUI in dev mode (Vite + Tauri)
	./dev-gui.sh

gui-release: ## Run GUI in dev mode with optimized Tauri and daemon binaries
	./dev-gui.sh --release

gui-package: ## Build DEB, RPM, and AppImage packages (usage: make gui-package V=0.1.12)
	bash scripts/release/package-gui-artifacts.sh $(V)

# ── Bench / Perf ───────────────────────────────────────

bench: ## Run benchmarks (usage: make bench LABEL=baseline)
	bash scripts/bench.sh run $(LABEL)

bench-compare: ## Compare last N bench runs (usage: make bench-compare N=5)
	bash scripts/bench.sh compare $(N)

perf: ## Run perf profiling (usage: make perf BACKEND=time LABEL=run PROFILE=substring-hit)
	bash scripts/perf.sh run $(BACKEND) $(LABEL) $(PROFILE)

perf-compare: ## Compare perf history (usage: make perf-compare BACKEND=time LABEL=run N=5)
	bash scripts/perf.sh compare $(BACKEND) $(LABEL) $(N)

# ── Release ────────────────────────────────────────────

set-version: ## Set workspace version (usage: make set-version V=0.2.0)
	bash scripts/release/set-version.sh $(V)

publish: ## Publish crates to crates.io (usage: make publish CHANNEL=stable)
	bash scripts/release/publish-crates.sh $(CHANNEL)

package: ## Package release artifacts (usage: make package V=0.1.11)
	bash scripts/release/package-artifacts.sh $(V)

verify-tag: ## Verify git tag matches workspace version (usage: make verify-tag TAG=v0.1.11)
	bash scripts/release/verify-tag.sh $(TAG)

next-beta: ## Print next beta tag (usage: make next-beta V=0.2.0)
	bash scripts/release/next-beta-tag.sh $(V)

changelog: ## Update changelog (usage: make changelog V=0.1.11 NOTES=notes.md)
	bash scripts/release/update-changelog.sh $(V) $(NOTES)
