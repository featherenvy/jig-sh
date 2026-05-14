# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

Jig turns a Rust application repo into an operating environment for coding agents. Without it, agents lose context across machines, lack a stable execution contract, and leave no inspectable record of their work. Jig fixes that by generating the scaffolding once and keeping it in sync.

It makes agentic software work repeatable, inspectable, and reviewable through:

- agent-facing repo guidance (`AGENTS.md`, `agent-map.md`)
- a stable top-level `make` contract
- a typed `jig` runtime over that contract
- a repo-scoped local dev proxy for stable development hostnames
- required work gates backed by receipts
- repo policy scripts
- GitHub Actions workflows
- template-based sync via the native `jig` renderer

## Prerequisites

- Rust 1.85+
- Bun — for repos with web app targets
- Postgres — when `sqlx_enabled = true`

## Installation

```sh
cargo install jig-sh
```

Generated repos install and pin their own `jig` version automatically via `scripts/install-jig.sh`. You only need a global install to run `jig init` or `jig adopt` on a repo for the first time.

## What It Generates

The template renders these repo-owned assets into a consumer repository:

- `.jig.toml`
- `.mcp.json`
- `AGENTS.md`
- `agent-map.md`
- `.agent/PLANS.md`
- `.agent/jig-contract.json`
- `Makefile`
- `scripts/*.sh`
- `scripts/enforce-coverage.js`
- `.github/workflows/*.yml`

Generated repos keep `make` as the execution backend, but they also get a `scripts/jig` launcher, MCP wiring, and append-only repo memory under `.agent/state/*.jsonl`.

On fresh machines, generated repos can check and bootstrap expected agent skills through the launcher:

```sh
scripts/jig agent doctor
scripts/jig agent bootstrap
```

For local dogfooding with an existing sibling `jig-skills` checkout, pass `--marketplace ../jig-skills` to `agent bootstrap`.

The template does not generate application code, crate-level `AGENTS.md` files, or a schema dump implementation — those remain project-owned. SQLx, migration, and schema-check contract pieces are optional via `sqlx_enabled`.

For existing repositories, root `AGENTS.md` remains repo-owned. `jig adopt` inserts or updates only the marked Jig-managed block and preserves the rest of the file.

## Quick Start

**Bootstrap a new repo:**

```sh
jig init /path/to/target-repo \
  --template /path/to/jig-sh \
  --template-mode committed \
  --repo-name target-repo \
  --rust-migration-dir migrations
```

For a tooling-only repo with no SQLx or migrations:

```sh
jig init /path/to/target-repo \
  --template /path/to/jig-sh \
  --template-mode committed \
  --repo-name target-repo \
  --sqlx-enabled false
```

**Adopt the template in an existing repository:**

```sh
cd /path/to/target-repo
jig adopt . \
  --template /path/to/jig-sh \
  --template-mode committed \
  --repo-name target-repo \
  --rust-migration-dir migrations
```

For a tooling-only repo, replace the migration flag with `--sqlx-enabled false`.

**Update an adopted repo:**

`jig update` refuses to overwrite changed template-managed files unless `--force` is passed:

```sh
cd /path/to/target-repo
jig update
```

For repos adopted from a local committed template checkout, update that checkout to the desired commit and run:

```sh
cd /path/to/target-repo
jig update \
  --template /path/to/jig-sh \
  --template-mode committed \
  --force
```

To re-render from stored `.jig.toml` answers without advancing the template source:

```sh
cd /path/to/target-repo
jig update --recopy
```

Pass `--force` if the rendered output should replace existing template-managed files.

`.jig.toml` serves as both the public repo-facing config and the renderer answers file used by `jig update --recopy`. `jig update --recopy` re-renders from the stored `_commit`; plain `jig update` advances to the current resolved template source. Set `template_source_url` in `.jig.toml` for portable recopy/update behavior across machines.

**Develop against a local build:**

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work status
```

## Templates

In Jig, `--template` means the source repository containing the harness files to render into another project. Today Jig ships one general-purpose repository harness template: this repo's `templates/project` directory.

Use a local checkout to dogfood head:

```sh
--template /path/to/jig-sh
```

Use the public git source to adopt from the shared template:

```sh
--template https://github.com/bpcakes/jig-sh.git
```

## Local Dev Proxy

Generated repos can run supervised development commands behind stable local hostnames.

### Running apps

```sh
scripts/jig dev
scripts/jig proxy alias api --port 8080
scripts/jig proxy list
```

`scripts/jig dev` runs configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or discovered workspace apps. It does not run the generic `dev_command`; keep `make dev` for repo-wide commands that do not bind a supervised app port. Prefer `argv` for `[[dev.apps]]`; shell-form `command` runs through the platform shell from committed repo configuration and should be treated as trusted code execution. Apps with `proxy = false` run directly and do not publish Jig proxy routes.

### HTTPS setup

Generate and explicitly trust the local CA:

```sh
scripts/jig proxy cert generate
scripts/jig proxy cert trust --accept-trust-scope
```

To remove trust before regenerating or discarding a CA:

```sh
scripts/jig proxy cert untrust --accept-trust-scope
```

The `--accept-trust-scope` flag is required to acknowledge platform trust-store mutation. The CA is local and name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses. Keep `ca-key.pem` private and exclude the proxy state directory from backup or sync tools.

Automatic certificate generation, trust, and untrust are supported on macOS and Linux. On macOS, untrust removes matching Jig CA certificates from the login keychain by fingerprint. On Linux, Jig invokes the p11-kit and CA-refresh helpers from fixed system tool directories. Windows HTTPS certificate files are not written until owner-only ACL hardening is implemented.

### Platform notes

Process-owned proxy routes are supported on Linux and macOS. LAN mode binds the proxy on `0.0.0.0`; reachable LAN clients can use process-owned routes to supervised loopback apps, while aliases remain loopback-client-only.

On Windows and BSD-like platforms, run app commands directly with `scripts/jig proxy run --no-proxy`, or use `scripts/jig proxy alias` for manually managed loopback services. Automatic process-owned route publication is not available on these platforms.

The `jig-sh` crate enables the proxy stack by default. Library or MCP/contract-only consumers that do not need TLS/HTTP proxy dependencies can build with `default-features = false`; the `dev` and `proxy` CLI surfaces remain parseable but return clear unsupported-feature errors.

## Required Repo Conventions

All generated repos are expected to use:

- Cargo workspaces
- `cargo fmt`
- `cargo clippy`

When `sqlx_enabled` is `true`, repos are also expected to use:

- SQLx workspace metadata in a shared directory such as `.sqlx/`
- forward-only migration additions

Optional web apps are expected to expose these package scripts in each configured app directory:

- `lint`
- `typecheck`
- `build:bundle`
- `test:coverage`

The default workflow assumes Bun for package installation and script execution.

## Layout

- `crates/jig/` — publishable `jig` runtime and MCP server
- `crates/jig-dev-proxy/` — local HTTP/HTTPS proxy with TLS certificate management
- `templates/project/` — files rendered into downstream repos
- `docs/` — configuration reference, adoption guide, and public contract documentation
- `examples/` — example `.jig.toml` answer files
- `scripts/validate-fixtures.sh` — renders sample repos and validates the generated harness

## Validate This Repo

```sh
./scripts/validate-fixtures.sh
```

## Release

Use the GitHub Actions `Release` workflow for the lowest-touch release path. Leave `version` blank to publish the next patch version, or set it explicitly. The workflow prepares the release commit, updates `CHANGELOG.md`, creates a local tag, publishes `jig-dev-proxy` and then `jig-sh` to crates.io through trusted publishing, pushes the tag to origin after both crates publish, and creates the GitHub Release.

`CHANGELOG.md` release sections are generated from git history. Conventional commit prefixes (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `perf:`, `build:`, `ci:`, `chore:`) drive the release-note categories; unprefixed commits land in `Other`. Do not hand-edit an upcoming version section before running the workflow.

<details>
<summary>Local release steps</summary>

The local release script is the typed entrypoint for validation and manual recovery. The `release-github` target requires the GitHub CLI (`gh`) with permission to create releases.

```sh
make release-prepare RELEASE_VERSION=0.1.1
ALLOW_DIRTY=1 make release-check RELEASE_VERSION=0.1.1
make release-stage
git commit -m "Release v0.1.1"
make release-check RELEASE_VERSION=0.1.1
make release-tag RELEASE_VERSION=0.1.1
make release-publish RELEASE_VERSION=0.1.1
make release-github RELEASE_VERSION=0.1.1
```

- `release-prepare` — updates all pinned version files and regenerates `CHANGELOG.md`
- `release-check` — requires a clean worktree, verifies version wiring and changelog coverage, runs `make ci`, validates rendered fixtures, and runs crates.io publish dry runs
- `release-tag` — creates the annotated local `vVERSION` tag after the same checks
- `release-publish` — requires the tag to point at `HEAD`, publishes `jig-dev-proxy`, waits for crates.io to see it, publishes `jig-sh`, then pushes the tag to origin
- `release-github` — creates the GitHub Release from the matching `CHANGELOG.md` section

Before the first split-crate release, pre-create crates.io Trusted Publishing configuration for both packages (`jig-dev-proxy` and `jig-sh`), repository `bpcakes/jig-sh`, workflow `release.yml`, and environment `crates-io`. Protect that GitHub environment with required reviewers.

`release-publish` skips package versions already present on crates.io and pushes the remote tag only after every crate is published. If only part of the crate set was published, keep the same version for remaining packages; bump only when a published crate version itself must change, since crates.io versions cannot be overwritten after yank.

If a workflow run pushes the release commit but fails before the tag is pushed, rerun the workflow with the explicit prepared version instead of leaving `version` blank.

For the already-published `v0.1.0`, run the workflow with `backfill_v0_1_0=true` to create the missing GitHub Release without publishing or retagging.

</details>

## License

MIT
