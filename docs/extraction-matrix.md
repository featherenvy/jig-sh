# Extraction Matrix

This matrix captures what was extracted from OneSales and how it was treated in `jig.sh`.

| Source asset | Treatment | Notes |
|---|---|---|
| `AGENTS.md` | Templated | Converted to generic repo-wide guidance with configurable paths and commands. |
| `agent-map.md` | Templated + generated | Rendered as a starter file, then refreshed by `scripts/generate-agent-map.sh`. |
| `.agent/PLANS.md` | Templated | Preserved as the generic ExecPlan contract. |
| `.agent/jig-contract.json` | Templated | Declares the make-backed repo contract for CLI and MCP consumers, with SQLx tools gated by `sqlx_enabled`. |
| `.agent/state/*.jsonl` | Runtime-owned | Append-only repo memory populated by `jig`. |
| `.mcp.json` | Templated | Repo-local MCP entrypoint that launches `scripts/jig mcp`. |
| `Makefile` | Templated subset | Kept the stable agent-facing command contract; removed OneSales-specific ops flows and now gates SQLx/migration targets behind `sqlx_enabled`. |
| `crates/jig` | Added | Publishable runtime that exposes the typed CLI/MCP surface over the generated make-backed contract and runtime-owned state. |
| `scripts/check-agent-map.sh` | Extracted | Mostly unchanged; remains generic. |
| `scripts/check-agent-guides.sh` | Extracted + simplified | Kept structural checks; removed OneSales wording rules. |
| `scripts/check-rust-file-loc.sh` | Extracted | Preserved as a generic repo policy check. |
| `scripts/check-migration-immutability.sh` | Extracted | Parameterized through rendered migration path and only kept when `sqlx_enabled` is `true`. |
| `scripts/generate-sqlx-unchecked-queries-todo.sh` | Extracted + generalized | Scans configurable crate roots and is only kept when `sqlx_enabled` is `true`. |
| `scripts/check-sqlx-unchecked-non-test.sh` | Extracted | Kept generic and only rendered for SQLx-enabled repos. |
| `scripts/check-schema-dump.sh` | Extracted + generalized | Calls a configurable schema dump command. |
| `scripts/add-migration.sh` | Templated | Adds timestamped forward-only migration stubs when `sqlx_enabled` is `true`. |
| `scripts/check-jig-contract.sh` | Templated | Validates runtime wiring and manifest drift. |
| `scripts/install-jig.sh` + `scripts/jig` | Templated | Exact-version runtime launcher and installer for generated repos. |
| `scripts/enforce-coverage.js` | Extracted | Kept generic. |
| `scripts/new-checkout.sh` | Extracted + generalized | Uses current repo basename instead of OneSales-specific naming. |
| `.github/workflows/agent-map-check.yml` | Templated | Runner label is configurable. |
| `.github/workflows/repo-policy.yml` | Templated subset | Keeps core policy checks and only includes SQLx/migration jobs when `sqlx_enabled` is `true`. |
| `.github/workflows/rust-tests.yml` | Templated subset | Simplified to generic fmt, clippy, and locked workspace tests. |
| `.github/workflows/webapp-checks-reusable.yml` + app workflows | Consolidated | Replaced with one generated matrix-style workflow or a disabled placeholder. |
| `scripts/dev.sh` | Excluded | Too application-specific. Downstream repos implement `dev_command`. |
| `scripts/dev-bootstrap.sh` | Excluded | App-specific onboarding/bootstrap logic. |
| OneSales crate guides | Excluded | Remain project-owned because ownership and entrypoints are repo-specific. |
