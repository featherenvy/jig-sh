# agentic-rust-kit

Reusable agentic-development kit for Rust application repos with a Tokio/Axum/SQLx/Postgres backend and optional web apps.

The kit extracts the durable parts of the OneSales workflow:

- agent-facing repo guidance
- a stable top-level `make` contract
- repo policy scripts
- GitHub Actions workflows
- template-based sync via `copier`

## What It Generates

The template renders these repo-owned assets into a consumer repository:

- `.agentic-kit.yaml`
- `AGENTS.md`
- `agent-map.md`
- `.agent/PLANS.md`
- `Makefile`
- `scripts/*.sh`
- `scripts/enforce-coverage.js`
- `.github/workflows/*.yml`

The template does not try to generate your application code, crate-level `AGENTS.md` files, or a schema dump implementation. Those remain project-owned.

## Quick Start

Render the kit into an existing repository:

```sh
uvx --from copier copier copy --trust /path/to/agentic-rust-kit /path/to/target-repo
```

If you edit `.agentic-kit.yaml` and want the repo to re-render from those answers:

```sh
cd /path/to/target-repo
uvx --from copier copier recopy --trust --defaults --answers-file .agentic-kit.yaml
```

To pull a newer version of the template while keeping the stored answers:

```sh
cd /path/to/target-repo
uvx --from copier copier update --trust --defaults --answers-file .agentic-kit.yaml
```

The generated repo uses `.agentic-kit.yaml` as both:

- the public repo-facing config
- the `copier` answers file used by `copier recopy` and `copier update`

When the template is rendered from a local checkout, the post-copy normalization step will try to replace a local `_src_path` with the template repo's `origin` remote URL. If the template checkout has no `origin` remote, `_src_path` remains local and update remains machine-local.

## Required Repo Conventions

Backend repos are expected to use:

- Cargo workspaces
- `cargo fmt`
- `cargo clippy`
- SQLx workspace metadata in a shared directory such as `.sqlx/`
- forward-only migration additions

Optional web apps are expected to expose these package scripts in each configured app directory:

- `lint`
- `typecheck`
- `build:bundle`
- `test:coverage`

The default workflow assumes Bun for package installation and script execution.

## Layout

- `copier.yml`: template configuration and questions
- `templates/project/`: files rendered into downstream repos
- `docs/`: config and adoption guidance
- `examples/`: example answer files
- `scripts/validate-fixtures.sh`: renders sample repos and validates the generated kit

## Validate This Repo

```sh
./scripts/validate-fixtures.sh
```
