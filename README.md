# jig.sh

Reusable agentic-development kit for Rust application repos, including SQLx/Postgres backends and tooling-only Rust repos, with optional web apps.

The kit extracts the durable parts of the OneSales workflow:

- agent-facing repo guidance
- a stable top-level `make` contract
- a typed `jig` runtime over that contract
- repo policy scripts
- GitHub Actions workflows
- template-based sync via `copier`

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

The template does not try to generate your application code, crate-level `AGENTS.md` files, or a schema dump implementation. Those remain project-owned. SQLx and migration-specific contract pieces are optional via `sqlx_enabled`.

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

For local dogfooding from an in-progress template checkout, use the working tree directly:

```sh
cd /path/to/target-repo
jig adopt . \
  --template /path/to/jig-sh \
  --template-mode working-tree \
  --repo-name target-repo \
  --sqlx-enabled false
```

Update an adopted repo while preserving local diffs when possible:

```sh
cd /path/to/target-repo
jig update
```

For repos adopted from a local working tree, `jig update` refreshes the repo-local snapshot automatically. To move back onto a clean committed template checkout, re-run update against that checkout:

```sh
cd /path/to/target-repo
jig update \
  --template /path/to/jig-sh \
  --template-mode committed
```

If you edit `.jig.yml` and want a full re-render from the stored answers:

```sh
cd /path/to/target-repo
jig update --recopy
```

The generated repo uses `.jig.yml` as both:

- the public repo-facing config
- the `copier` answers file used under the hood by `jig update` and `jig update --recopy`

`jig` shells out to `uvx --from copier copier ...`, so direct Copier usage remains available as a fallback:

```sh
uvx --from copier copier copy --trust /path/to/jig-sh /path/to/target-repo
uvx --from copier copier update --trust --answers-file .jig.yml
uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml
```

Set `template_source_url` in `.jig.yml` if you want portable recopy/update behavior across machines. The value is validated before `_src_path` is rewritten: it must be fetchable with git, and the current `_commit` must already be reachable from `refs/heads/<default_branch>` at that source.

Without `template_source_url`, the post-copy normalization step only rewrites `_src_path` from a local checkout path to the template repo's `origin` URL when the current `_commit` is already contained in the local `origin/<default_branch>` tracking ref. Otherwise it keeps the local path to avoid stamping an unreachable remote commit. Repos adopted with `--template-mode working-tree` stay local-first on purpose: they keep a repo-local template snapshot and skip remote rewrite until you relink them to a committed template source.

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

- `copier.yml`: template configuration and questions
- `crates/jig/`: publishable `jig` runtime and MCP server
- `templates/project/`: files rendered into downstream repos
- `docs/`: config and adoption guidance
- `examples/`: example answer files
- `scripts/validate-fixtures.sh`: renders sample repos and validates the generated kit

## Validate This Repo

```sh
./scripts/validate-fixtures.sh
```
