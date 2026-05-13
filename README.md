# jig.sh

Reusable harness for making Rust application repos operable by coding agents, including SQLx/Postgres backends and tooling-only Rust repos, with optional web apps.

Jig turns a repo into an operating environment for coding agents. It makes agentic software work repeatable, inspectable, and reviewable through:

- agent-facing repo guidance
- a stable top-level `make` contract
- a typed `jig` runtime over that contract
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

Use the GitHub Actions `Release` workflow for the lowest-touch release path. Leave `version` blank to publish the next patch version, or set it explicitly. The workflow prepares the release commit, updates `CHANGELOG.md`, tags the commit, publishes `jig-sh` to crates.io through trusted publishing, and creates the GitHub Release.

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

`release-prepare` updates all pinned version files and regenerates `CHANGELOG.md`; run it before `release-tag` when bumping versions locally. `release-check` requires a clean worktree, verifies repo version wiring and changelog coverage, runs `make ci`, validates rendered fixtures, and runs a crates.io publish dry run. `release-tag` creates the annotated `vVERSION` tag after the same checks. `release-publish` first requires that tag to point at `HEAD`, then reruns the full checks, pushes the tag to `origin` if needed, verifies the remote tag resolves to `HEAD`, and publishes `jig-sh` to crates.io. `release-github` creates the GitHub Release from the matching `CHANGELOG.md` section.

The GitHub workflow expects crates.io Trusted Publishing to be configured for package `jig-sh`, repository `bpcakes/jig-sh`, workflow `release.yml`, and environment `crates-io`. If publishing fails after the tag is pushed, fix the registry/auth/network issue and rerun the publish step or `make release-publish`; it will reuse the existing remote tag when it still points at `HEAD`. If crates.io rejects the crate contents, bump the version before trying again because the release tag for the rejected version is already public.

If a workflow run pushes the release commit but fails before the tag is pushed, rerun the workflow with the explicit prepared version instead of leaving `version` blank.
