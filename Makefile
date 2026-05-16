SHELL := /bin/bash

.DEFAULT_GOAL := help

CARGO ?= cargo
NODE ?= node
BUN ?= bun
BUN_INSTALL_FLAGS ?= --frozen-lockfile

# This source repository still uses master; generated Makefiles render their
# own DEFAULT_BRANCH from the downstream repo's .jig.toml.
DEFAULT_BRANCH ?= master
JIG_VERSION ?= 0.2.0-beta.1
RUST_CRATE_ROOTS := crates

# Source-repo convenience target; generated repos set this from .jig.toml.
DEV_COMMAND := cargo test --workspace

.PHONY: help bootstrap deps dev fmt-check clippy test-rust test-rust-locked test

.PHONY: contract-check

.PHONY: check-agent-map check-agent-guides check-rust-file-loc check-no-mod-rs check-launcher-template
.PHONY: enforce-coverage release-notes release-prepare release-stage release-check release-tag release-publish release-github ci ci-webapps


help: ## Show all available targets
	@echo "jig-sh Makefile"
	@awk 'BEGIN {FS = ":.*##"} /^[A-Za-z0-9._-]+:.*##/ {printf "  %-28s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# Bootstrap uses a login shell so PATH includes tools installed by shell profile setup.
bootstrap: ## Initialize the local workspace
	@echo "If bootstrap cannot start jig, run: make deps"
	@bash -lc 'scripts/jig bootstrap'

# Keep this as a pre-jig primitive so dependency fetch still works while the
# launcher or local jig binary is being repaired.
deps: ## Install Rust and optional webapp dependencies
	$(CARGO) fetch

	@echo "No web apps configured."


dev: ## Run the configured dev command
	@bash -lc '$(DEV_COMMAND)'

fmt-check: ## Check Rust formatting
	scripts/jig check fmt

clippy: ## Run clippy for the Rust workspace
	scripts/jig check clippy

test-rust: ## Run Rust workspace tests
	scripts/jig check test

test-rust-locked: ## Run Rust workspace tests with --locked
	scripts/jig check test-locked

test: test-rust ## Run the default backend test suite

contract-check: ## Verify the generated jig contract and runtime wiring
	scripts/jig check contract

check-agent-map: ## Verify agent-map.md coverage and links
	scripts/jig check agent-map

check-agent-guides: ## Verify crate-level AGENTS.md guides
	scripts/jig check agent-guides

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
	scripts/jig check rust-file-loc --changed-against "$$base_ref"

check-no-mod-rs: ## Fail if disallowed mod.rs files exist under configured crate roots
	scripts/jig check no-mod-rs

check-launcher-template: ## Verify source and rendered launcher templates stay synchronized
	@set -euo pipefail; \
	normalize_launcher() { \
	  sed \
	    -e '/^# Keep launcher behavior synchronized /,+1d' \
	    -e '/^\[% if repo_name == "jig-sh" %\]$$/d' \
	    -e '/^\[% endif %\]$$/d' \
	    -e 's/^JIG_VERSION="[^"]*"$$/JIG_VERSION="<<[ jig_version ]>>"/' \
	    "$$1"; \
	}; \
	if ! diff -u <(normalize_launcher scripts/jig) <(normalize_launcher templates/project/scripts/jig.jinja); then \
	  echo "scripts/jig and templates/project/scripts/jig.jinja drifted." >&2; \
	  exit 1; \
	fi



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

release-publish: ## Validate tagged HEAD, publish crates, then push tag to origin
	scripts/release.sh publish $(RELEASE_VERSION)

release-github: ## Create the GitHub Release for vVERSION from CHANGELOG.md
	scripts/release.sh github $(RELEASE_VERSION)


ci-webapps: ## No configured web apps
	@echo "No web apps configured."


ci: fmt-check clippy test-rust-locked contract-check check-rust-file-loc check-no-mod-rs check-agent-map check-agent-guides check-launcher-template ci-webapps ## Run the standard CI validation set
