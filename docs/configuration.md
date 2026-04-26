# `.jig.yml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

`.jig.yml` is also the `copier` answers file.

After changing values in `.jig.yml`, re-render with:

```sh
jig update --recopy
```

To move onto a newer version of the template while keeping the stored answers, run:

```sh
jig update
```

The file contains both public settings and the private `_src_path` / `_commit` fields that `copier update` requires. Local-template repos may also store `_template_mode` and `_template_local_path` for `jig`'s local snapshot handling.

`jig` shells out to Copier via `uvx --from copier copier ...`. Direct Copier usage remains available if needed:

```sh
uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml
uvx --from copier copier update --trust --answers-file .jig.yml
```

For local git template checkouts, `jig init` / `jig adopt` require:

- `--template-mode committed`: use the clean local `HEAD`
- `--template-mode working-tree`: snapshot the exact current checkout, including uncommitted changes

## Required Keys

- `repo_name`: display name used in generated docs
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: runner label for GitHub Actions jobs
- `jig_version`: exact runtime version expected by generated repos
- `template_source_url`: optional canonical template source URL for portable recopy/update
- `sqlx_enabled`: whether to generate SQLx and migration-specific contract pieces
- `rust_crate_roots`: list of directories whose direct child directories are considered crates

When `sqlx_enabled` is `true`, these additional keys are required:

- `rust_migration_dir`: SQL migration directory
- `rust_sqlx_metadata_dir`: committed SQLx metadata directory

## Optional Keys

- `schema_dump_enabled`: when `true` and `sqlx_enabled` is also `true`, `make schema-check` executes `schema_dump_command`
- `schema_dump_command`: command that regenerates schema docs for SQLx-enabled repos
- `migration_add_command`: command behind `make migration-add` when `sqlx_enabled` is `true`
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

The compatibility policy for generated CLI commands, MCP tools, `.agent/jig-contract.json`, and `.agent/state/*.jsonl` is defined in [Public Contract](./public-contract.md).

The generated `Makefile` exposes these stable targets:

- `bootstrap`
- `deps`
- `dev`
- `fmt-check`
- `clippy`
- `test-rust`
- `test-rust-locked`
- `test`
- `contract-check`
- `check-agent-map`
- `check-agent-guides`
- `check-rust-file-loc`
- `check-no-mod-rs`
- `ci`

When `sqlx_enabled` is `true`, generated repos also expose:

- `sqlx-db-setup`
- `sqlx-check`
- `schema-check`
- `migration-add`
- `check-sqlx-unchecked-non-test`

When both `sqlx_enabled` and `schema_dump_enabled` are `true`, generated repos also expose:

- `schema-dump`

Downstream repos may add more targets, but these names should remain stable for agent tooling.

Generated repos also get these runtime-owned files:

- `.mcp.json`
- `.agent/jig-contract.json`
- `scripts/jig`
- `scripts/install-jig.sh`

The generated `scripts/jig` launcher enforces the exact `jig_version` pinned in `.jig.yml`. On first use it installs that version into a repo-local cache and then exposes the same contract as:

- CLI commands such as `scripts/jig fmt-check`
- MCP tools such as `jig.fmt_check`
- append-only memory under `.agent/state/*.jsonl`

For local runtime development, set `JIG_DEV_BIN` to an already-built `jig` binary. The installer uses that explicit binary before any cached exact-version binary, while still verifying that its reported version matches `.jig.yml`.

## SQLx Metadata Directory

This section applies only when `sqlx_enabled` is `true`.

`rust_sqlx_metadata_dir` is wired into the generated `sqlx-check` target via `SQLX_OFFLINE_DIR`. Use `.sqlx` unless the repository has already standardized on a different committed metadata path.

## Template Source

For portable shared repos, set:

```yaml
template_source_url: git@github.com:your-org/jig-sh.git
```

When `template_source_url` is set, the generated normalization step validates it before writing `_src_path`:

- the source must be fetchable with `git`
- `refs/heads/<default_branch>` must exist there
- the current `_commit` must already be reachable from that branch history

If any of those checks fail, `copier` exits instead of saving an unusable remote template source into `.jig.yml`.

If `template_source_url` is blank, the post-copy normalization step may rewrite a local `_src_path` to the template repo's `origin` URL, but only when the current `_commit` is already reachable from the local `origin/<default_branch>` tracking ref. Otherwise it leaves the local path unchanged to avoid recording an unreachable remote commit.

When `_template_mode` is `working-tree`, `jig` stores a repo-local git snapshot under `.agent/.cache/template-source` and skips remote rewrite. That keeps the adopted repo updateable from the exact local template checkout until you intentionally relink it to a committed source.
