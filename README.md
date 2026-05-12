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

- `.jig.yml`
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

If you edit `.jig.yml` and want a full re-render from the stored answers:

```sh
cd /path/to/target-repo
jig update --recopy
```

If the rendered output should replace existing template-managed files, pass `--force`.

The generated repo uses `.jig.yml` as both:

- the public repo-facing config
- the native renderer answers file used by `jig update` and `jig update --recopy`

Set `template_source_url` in `.jig.yml` if you want portable recopy/update behavior across machines. When set, the renderer writes it into `_src_path`; otherwise local template renders keep the local source path.

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

Use the release script as the single local entrypoint for release validation, tagging, and publishing:

```sh
make release-check
make release-tag
make release-publish
```

The release version defaults to the `jig-sh` package version from Cargo metadata. To override it explicitly:

```sh
make release-check RELEASE_VERSION=0.1.0
```

`release-check` requires a clean worktree, verifies repo version wiring, runs `make ci`, validates rendered fixtures, and runs a crates.io publish dry run. `release-tag` creates the annotated `vVERSION` tag after the same checks. `release-publish` first requires that tag to point at `HEAD`, then reruns the full checks, pushes the tag to `origin` if needed, verifies the remote tag resolves to `HEAD`, and publishes `jig-sh` to crates.io.

Before running `release-publish`, authenticate cargo with `cargo login` or `CARGO_REGISTRY_TOKEN`. If publishing fails after the tag is pushed, fix the registry/auth/network issue and rerun `make release-publish`; it will reuse the existing remote tag when it still points at `HEAD`. If crates.io rejects the crate contents, bump the version before trying again because the release tag for the rejected version is already public.
