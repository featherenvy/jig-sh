# `.jig.yml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

`.jig.yml` is also the native renderer answers file.

After changing values in `.jig.yml`, re-render with:

```sh
jig update --recopy
```

To move onto a newer version of the template while keeping the stored answers, run:

```sh
jig update
```

The file contains both public settings and the private `_src_path` / `_commit` fields that `jig update` uses to resolve future renders. Repos rendered from local committed template checkouts may also store `_template_mode` and `_template_local_path`.

`jig update` refuses to overwrite or remove changed template-managed files unless `--force` is passed.

Root `AGENTS.md` is block-managed instead of file-managed. If the file already exists, `jig adopt` and `jig update` preserve user-authored content and insert or replace only the section between `<!-- BEGIN JIG MANAGED BLOCK -->` and `<!-- END JIG MANAGED BLOCK -->`. Edits inside that managed block are template-owned and may be replaced without `--force`; keep repo-specific guidance outside the markers.

For local git template checkouts, `jig init` / `jig adopt` use a committed source:

- `--template-mode committed`: explicitly use the clean local `HEAD`
- omit `--template-mode`: use the same committed local-template behavior

## Required Keys

- `repo_name`: display name used in generated docs
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: runner label for GitHub Actions jobs
- `jig_version`: exact runtime version expected by generated repos
- `work.gates`: required work evidence gates evaluated before `scripts/jig work finish`
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

## `work` Shape

The `work` block declares agent workflow defaults without adding repo-local launcher scripts:

```yaml
work:
  gates:
    - id: contract
      kind: check
      tool: jig.contract_check
    - id: tests
      kind: check
      tool: jig.test
```

`kind: check` gates must reference make-backed jig tool names declared in `.agent/jig-contract.json`. `scripts/jig work check --plan-id ...` runs configured check gates in order unless one or more `--tool` values are passed explicitly.

`scripts/jig work gates --plan-id ...` reports each configured gate as `passed`, `missing`, `failed`, `stale`, `unknown`, or `unsupported`. `scripts/jig work finish --plan-id ...` refuses to close work while required gates are missing, failed, stale, unknown, or unsupported. Check gate freshness is based on the non-`.agent/` worktree fingerprint from the latest check or check-batch receipt that proves the gate.

Required check gates should not create or modify non-`.agent/` files during `work check`. Build outputs, generated metadata, and lockfiles should be committed when they are source-of-truth, ignored when they are disposable, or generated before running the fingerprinted check. If a check does intentionally settle generated files, rerun `scripts/jig work check --plan-id ...` after reviewing those changes so the gate evidence matches the final worktree.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint`, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older successful check receipts deserialize correctly, but their freshness is `unknown` and required gates will block finish until fresh evidence exists.

For compatibility, older repos may still use `work.checks`; Jig backfills entries that are not already declared in `work.gates` as required `kind: check` gates with generated IDs. When a tool is declared in both places, the explicit `work.gates` entry is authoritative. New repos should use `work.gates`.

Generated SQLx-enabled repos also include check gates for `jig.sqlx_check` and `jig.schema_check`. Repos with schema dumps enabled also include `jig.schema_dump`.

Review procedures are intentionally separate from native check gates:

```yaml
work:
  gates:
    - id: rust-error-handling
      kind: codex_review
      skill: jig-rust:rust-error-handling-review
      required: false
```

Codex-backed review gates are not implemented yet. They require a structured `codex exec --output-schema ...` receipt path before they can be required. Until then, non-`check` gates are reported as `unsupported` and block finish only when marked `required: true`.

`work.refinements` is reserved for future refinement execution. Current Jig versions reject it with a clear configuration error instead of accepting no-op refinement entries.

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

The compatibility policy for generated make-backed CLI commands, MCP tools, and `.agent/jig-contract.json` is defined in [Public Contract](./public-contract.md).

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

The generated `scripts/jig` launcher enforces the exact `jig_version` pinned in `.jig.yml`. On first use it installs that version into a repo-local cache and then exposes the make-backed contract as:

- CLI commands such as `scripts/jig fmt-check`
- MCP tools such as `jig.fmt_check`

It also provides runtime-owned append-only memory under `.agent/state/*.jsonl` through the structured work namespace:

- `scripts/jig work start --title ...`
- `scripts/jig work append --plan-id ...`
- `scripts/jig work check --plan-id ...`
- `scripts/jig work gates --plan-id ...`
- `scripts/jig work decide --plan-id ...`
- `scripts/jig work receipts --plan-id ...`
- `scripts/jig work status`
- `scripts/jig work finish --plan-id ...`

`work finish` closes the plan with `--resolution`. If an active session is also open, it closes that session with `--outcome`; when `--outcome` is omitted, the session outcome falls back to `--resolution`.

For local runtime development, set `JIG_DEV_BIN` to an already-built `jig` binary. The installer uses that explicit binary before any cached exact-version binary, while still verifying that its reported version matches `.jig.yml`.

## SQLx Metadata Directory

This section applies only when `sqlx_enabled` is `true`.

`rust_sqlx_metadata_dir` is wired into the generated `sqlx-check` target via `SQLX_OFFLINE_DIR`. Use `.sqlx` unless the repository has already standardized on a different committed metadata path.

## Template Source

For portable shared repos, set:

```yaml
template_source_url: git@github.com:your-org/jig-sh.git
```

When `template_source_url` is set, the renderer writes it into `_src_path` for portable update and install behavior. If it is blank, local template renders keep the local source path.
