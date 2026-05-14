# jig.sh

Reusable harness for making Rust application repos operable by coding agents, including SQLx/Postgres backends and tooling-only Rust repos, with optional web apps.

Jig turns a repo into an operating environment for coding agents. It makes agentic software work repeatable, inspectable, and reviewable through:

- agent-facing repo guidance
- a stable top-level `make` contract
- a typed `jig` runtime over that contract
- a repo-scoped local dev proxy for stable development hostnames
- required work gates backed by receipts
- repo policy scripts
- GitHub Actions workflows
- template-based sync via the native `jig` renderer

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

On fresh machines, generated repos can check and bootstrap expected Codex-side Jig skills through the launcher:

```sh
scripts/jig agent doctor
scripts/jig agent bootstrap
```

For local dogfooding with an existing sibling `jig-skills` checkout, pass `--marketplace ../jig-skills` to `agent bootstrap`.

The template does not try to generate your application code, crate-level `AGENTS.md` files, or a schema dump implementation. Those remain project-owned. SQLx, migration, and schema-check contract pieces are optional via `sqlx_enabled`.

For existing repositories, root `AGENTS.md` remains repo-owned. `jig adopt` inserts or updates only the marked Jig managed block and preserves the rest of the file.

## Templates

In Jig, `--template` means the source repository that contains the harness files
to render into another project. Today Jig ships one general-purpose repository
harness template: this `jig-sh` repo's `templates/project` directory.

Use a local `jig-sh` checkout when you want to dogfood head:

```sh
--template /path/to/jig-sh
```

Use the public git source when you want to adopt from the shared template:

```sh
--template https://github.com/bpcakes/jig-sh.git
```

## Quick Start

Bootstrap a new repo from the template:

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

Adopt the template in an existing repository:

```sh
cd /path/to/target-repo
jig adopt . \
  --template /path/to/jig-sh \
  --template-mode committed \
  --repo-name target-repo \
  --rust-migration-dir migrations
```

For a tooling-only repo, replace the migration flag with `--sqlx-enabled false`.

Update an adopted repo. `jig update` refuses to overwrite changed template-managed files unless `--force` is passed:

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

When changing the `jig` runtime itself, build a dev binary and point the launcher at it so the repo-local cache cannot mask current code:

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work status
```

## Local Dev Proxy

Generated repos can run supervised development commands behind stable local hostnames:

```sh
scripts/jig dev
scripts/jig proxy alias api --port 8080
scripts/jig proxy list
```

`scripts/jig dev` runs configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or discovered workspace apps. It does not run the generic `dev_command`; keep `make dev` for repo-wide commands that do not bind a supervised app port. Prefer `argv` for `[[dev.apps]]`; shell-form `command` runs through the platform shell from committed repo configuration and should be treated as trusted code execution. Apps with `proxy = false` run directly and do not publish Jig proxy routes.

For HTTPS browser trust, generate and explicitly trust the local CA:

```sh
scripts/jig proxy cert generate
scripts/jig proxy cert trust --accept-trust-scope
scripts/jig proxy cert untrust --accept-trust-scope
```

The trusted CA is local and name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses, but the trust and untrust commands still require `--accept-trust-scope` to acknowledge platform trust-store mutation. Keep `ca-key.pem` private, exclude the proxy state directory from backup or sync tools that may copy private keys outside local filesystem permissions, and run `scripts/jig proxy cert untrust --accept-trust-scope` before regenerating or discarding a trusted CA. On macOS, untrust removes matching Jig CA certificates from the login keychain by fingerprint; on Linux, Jig invokes the p11-kit and CA-refresh helpers from fixed system tool directories, and untrust removes the exact current CA trust anchor when available. Automatic certificate generation, trust, and untrust are supported on macOS and Linux; Windows HTTPS certificate files are not written until owner-only ACL hardening is implemented.

Process-owned proxy routes are supported on Linux and macOS. LAN mode binds the proxy on `0.0.0.0`; reachable LAN clients can use process-owned routes to supervised loopback apps, while aliases remain loopback-client-only. On Windows and BSD-like platforms, Jig can still run app commands directly with `scripts/jig proxy run --no-proxy`, or you can use `scripts/jig proxy alias` for manually managed loopback services, but automatic process-owned route publication is refused until high-confidence process start-token verification is available. Windows `proxy stop` uses the authenticated health PID but cannot start-token-check that PID before `taskkill`.

The `jig-sh` crate enables the local proxy stack by default. Library or MCP/contract-only consumers that do not need the TLS/HTTP proxy dependencies can build with `default-features = false`; the `dev` and `proxy` CLI surfaces remain parseable but return clear unsupported-feature errors.

If you edit `.jig.toml` and want a full re-render from the stored answers:

```sh
cd /path/to/target-repo
jig update --recopy
```

If the rendered output should replace existing template-managed files, pass `--force`.

The generated repo uses `.jig.toml` as both:

- the public repo-facing config
- the native renderer answers file used by `jig update` and `jig update --recopy`

Set `template_source_url` in `.jig.toml` if you want portable recopy/update behavior across machines. When set, the renderer writes it into `_src_path`; otherwise local template renders keep the local source path.

`jig update --recopy` re-renders from the stored `_commit`. Plain `jig update` advances to the current resolved template source.

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

Generated repos also expect Rust to be available for `scripts/install-jig.sh`, which installs the exact pinned `jig` version when the launcher is first used.

## Layout

- `crates/jig/`: publishable `jig` runtime and MCP server
- `templates/project/`: files rendered into downstream repos
- `docs/`: config, adoption, and public-contract guidance
- `examples/`: example answer files
- `scripts/validate-fixtures.sh`: renders sample repos and validates the generated harness

## Validate This Repo

```sh
./scripts/validate-fixtures.sh
```

## Release Jig

Use the GitHub Actions `Release` workflow for the lowest-touch release path. Leave `version` blank to publish the next patch version, or set it explicitly. The workflow prepares the release commit, updates `CHANGELOG.md`, creates a local tag, publishes `jig-dev-proxy` and then `jig-sh` to crates.io through trusted publishing, pushes the tag to origin after both crates publish, and creates the GitHub Release.

For the already-published `v0.1.0`, run the same workflow with `backfill_v0_1_0=true` to create the missing GitHub Release without publishing or retagging.

`CHANGELOG.md` release sections are generated from git history and owned by the release automation. Conventional commit prefixes (`feat:`, `fix:`, `docs:`, `test:`, `tests:`, `refactor:`, `perf:`, `build:`, `ci:`, `chore:`) drive the release-note categories; unprefixed commits land in `Other`. Do not hand-edit an upcoming version section before running the workflow; put any wording changes in a follow-up commit after the generated release notes exist.

The local release script remains the typed entrypoint for validation and manual recovery. The `release-github` target requires the GitHub CLI (`gh`) with permission to create releases.

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

The release version defaults to the `jig-sh` package version from Cargo metadata. To override it explicitly:

```sh
make release-check RELEASE_VERSION=0.1.0
```

`release-prepare` updates all pinned version files and regenerates `CHANGELOG.md`; run it before `release-tag` when bumping versions locally. `release-check` requires a clean worktree, verifies repo version wiring and changelog coverage, runs `make ci`, validates rendered fixtures, and runs crates.io publish dry runs where the registry dependency chain allows them. Before a new `jig-dev-proxy` version exists in the registry, `release-check` validates the `jig-sh` package with a local registry patch so the package is still built before any version is published. `release-tag` creates the annotated local `vVERSION` tag after the same checks. `release-publish` first requires that tag to point at `HEAD`, reruns the full checks, publishes `jig-dev-proxy`, waits for crates.io to see it, publishes `jig-sh`, then pushes the tag to `origin` if needed and verifies the remote tag resolves to `HEAD`. `release-github` creates the GitHub Release from the matching `CHANGELOG.md` section.

Before the first split-crate release, pre-create crates.io Trusted Publishing configuration for both packages: `jig-dev-proxy` and `jig-sh`, repository `bpcakes/jig-sh`, workflow `release.yml`, and environment `crates-io`; protect that GitHub environment with required reviewers. If either package is not registered for trusted publishing, the first publish attempt for that package fails before the release tag is pushed. Fix the registry/auth/network issue and rerun the publish step or `make release-publish`; it will skip package versions already present on crates.io and push the remote tag only after every crate is published. If crates.io rejects a package before any crate version is published, fix the release commit and rerun with the explicit prepared version. If only part of the crate set was published, keep the same version for the remaining package when the published crate contents are acceptable; bump only when a published crate version itself must change because crates.io versions cannot be overwritten or republished after yank.

If a workflow run pushes the release commit but fails before the tag is pushed, rerun the workflow with the explicit prepared version instead of leaving `version` blank.
