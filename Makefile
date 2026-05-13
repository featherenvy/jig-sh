SHELL := /bin/bash

.DEFAULT_GOAL := help

CARGO ?= cargo
NODE ?= node
BUN ?= bun
BUN_INSTALL_FLAGS ?= --frozen-lockfile

DEFAULT_BRANCH ?= main
JIG_VERSION ?= 0.1.0
RUST_CRATE_ROOTS := crates

BOOTSTRAP_COMMAND := cargo fetch
DEV_COMMAND := cargo test --workspace

.PHONY: help bootstrap deps dev fmt-check clippy test-rust test-rust-locked test

.PHONY: contract-check

.PHONY: check-agent-map check-agent-guides check-rust-file-loc check-no-mod-rs
.PHONY: enforce-coverage release-notes release-prepare release-stage release-check release-tag release-publish release-github ci ci-webapps


help: ## Show all available targets
	@echo "jig-sh Makefile"
	@awk 'BEGIN {FS = ":.*##"} /^[A-Za-z0-9._-]+:.*##/ {printf "  %-28s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

bootstrap: ## Initialize the local workspace
	@bash -lc '$(BOOTSTRAP_COMMAND)'

deps: ## Install Rust and optional webapp dependencies
	$(CARGO) fetch

	@echo "No web apps configured."


dev: ## Run the local development stack
	@bash -lc '$(DEV_COMMAND)'

fmt-check: ## Check Rust formatting
	cargo fmt --all -- --check

clippy: ## Run clippy for the Rust workspace
	cargo clippy --workspace --all-targets --locked -- -D warnings

test-rust: ## Run Rust workspace tests
	cargo test --workspace

test-rust-locked: ## Run Rust workspace tests with --locked
	cargo test --workspace --locked

test: test-rust ## Run the default backend test suite

contract-check: ## Verify the generated jig contract and runtime wiring
	scripts/check-jig-contract.sh

check-agent-map: ## Verify agent-map.md coverage and links
	scripts/check-agent-map.sh

check-agent-guides: ## Verify crate-level AGENTS.md guides
	scripts/check-agent-guides.sh

check-rust-file-loc: ## Enforce Rust file-size policy against the default branch
	@set -euo pipefail; \
	if git rev-parse --verify origin/$(DEFAULT_BRANCH) >/dev/null 2>&1; then \
	  base_ref="$$(git merge-base HEAD origin/$(DEFAULT_BRANCH))"; \
	elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then \
	  base_ref="HEAD^"; \
	else \
	  base_ref="4b825dc642cb6eb9a060e54bf8d69288fbee4904"; \
	fi; \
	echo "Using Rust LOC base ref: $$base_ref"; \
	scripts/check-rust-file-loc.sh --changed-against "$$base_ref"

check-no-mod-rs: ## Fail if disallowed mod.rs files exist under configured crate roots
	@set -euo pipefail; \
	violations=""; \
	for root in $(RUST_CRATE_ROOTS); do \
	  matches="$$(git ls-files "$$root/**/mod.rs" 2>/dev/null || true)"; \
	  if [ -n "$$matches" ]; then \
	    violations="$$violations $$matches"; \
	  fi; \
	done; \
	if [ -n "$$violations" ]; then \
	  echo "Disallowed Rust module file(s) found. Use named module files instead of mod.rs."; \
	  printf '%s\n' $$violations; \
	  exit 1; \
	fi; \
	echo "No disallowed mod.rs files found under configured crate roots."



enforce-coverage: ## Enforce a coverage threshold from COVERAGE_THRESHOLD against COVERAGE_DIR
	@COVERAGE_DIR="$(COVERAGE_DIR)" COVERAGE_THRESHOLD="$(COVERAGE_THRESHOLD)" $(NODE) scripts/enforce-coverage.js

release-notes: ## Generate CHANGELOG.md notes for RELEASE_VERSION
	scripts/release.sh notes $(RELEASE_VERSION)

release-prepare: ## Update pinned versions and CHANGELOG.md for RELEASE_VERSION
	scripts/release.sh prepare $(RELEASE_VERSION)

release-stage: ## Stage files updated by release-prepare
	scripts/release.sh stage

release-check: ## Run local release validation and cargo publish dry run
	scripts/release.sh check $(RELEASE_VERSION)

release-tag: ## Validate and create annotated release tag vVERSION
	scripts/release.sh tag $(RELEASE_VERSION)

release-publish: ## Validate tagged HEAD, push tag to origin, and publish jig-sh
	scripts/release.sh publish $(RELEASE_VERSION)

release-github: ## Create the GitHub Release for vVERSION from CHANGELOG.md
	scripts/release.sh github $(RELEASE_VERSION)


ci-webapps: ## No configured web apps
	@echo "No web apps configured."


ci: fmt-check clippy test-rust-locked contract-check check-rust-file-loc check-no-mod-rs check-agent-map check-agent-guides ci-webapps ## Run the standard CI validation set
