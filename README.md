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

- `.copier-answers.yml`
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

Or update a repo that already uses the kit:

```sh
cd /path/to/target-repo
uvx --from copier copier update --trust
```

The generated repo includes two committed configuration files:

- `.agentic-kit.yaml`: public repo-facing config
- `.copier-answers.yml`: `copier` sync state used by `copier update`

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
