# `.agentic-kit.yaml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

`.agentic-kit.yaml` is also the `copier` answers file. Run updates with:

```sh
uvx --from copier copier update --trust --defaults --answers-file .agentic-kit.yaml
```

The file contains both public settings and the private `_src_path` / `_commit` fields that `copier update` requires.

## Required Keys

- `repo_name`: display name used in generated docs
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: runner label for GitHub Actions jobs
- `rust_crate_roots`: list of directories whose direct child directories are considered crates
- `rust_migration_dir`: SQL migration directory
- `rust_sqlx_metadata_dir`: committed SQLx metadata directory

## Optional Keys

- `schema_dump_enabled`: when `true`, `make schema-check` executes `schema_dump_command`
- `schema_dump_command`: command that regenerates schema docs
- `bootstrap_command`: implementation behind `make bootstrap`
- `dev_command`: implementation behind `make dev`
- `rust_fmt_check_command`
- `rust_clippy_command`
- `rust_test_command`
- `rust_test_locked_command`
- `web_package_manager`: currently `bun`
- `frontend_apps`: list of app definitions

## `frontend_apps` Shape

Each entry in `frontend_apps` must be an object:

```yaml
frontend_apps:
  - name: frontend
    dir: frontend
    coverage_threshold: 40
  - name: admin-panel
    dir: admin-panel
    coverage_threshold: 0
```

Each configured app directory is expected to support:

- install: `bun install --frozen-lockfile`
- lint: `bun run lint`
- typecheck: `bun run typecheck`
- build: `bun run build:bundle`
- test coverage: `bun run test:coverage`

## Generated Contract

The generated `Makefile` exposes these stable targets:

- `bootstrap`
- `deps`
- `dev`
- `fmt-check`
- `clippy`
- `test-rust`
- `test-rust-locked`
- `test`
- `sqlx-db-setup`
- `sqlx-check`
- `schema-check`
- `check-agent-map`
- `check-agent-guides`
- `check-rust-file-loc`
- `check-no-mod-rs`
- `check-sqlx-unchecked-non-test`
- `ci`

Downstream repos may add more targets, but these names should remain stable for agent tooling.

## SQLx Metadata Directory

`rust_sqlx_metadata_dir` is wired into the generated `sqlx-check` target via `SQLX_OFFLINE_DIR`. Use `.sqlx` unless the repository has already standardized on a different committed metadata path.
