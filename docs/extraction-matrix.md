# Extraction Matrix

This matrix captures what was extracted from OneSales and how it was treated in `agentic-rust-kit`.

| Source asset | Treatment | Notes |
|---|---|---|
| `AGENTS.md` | Templated | Converted to generic repo-wide guidance with configurable paths and commands. |
| `agent-map.md` | Templated + generated | Rendered as a starter file, then refreshed by `scripts/generate-agent-map.sh`. |
| `.agent/PLANS.md` | Templated | Preserved as the generic ExecPlan contract. |
| `Makefile` | Templated subset | Kept the stable agent-facing command contract; removed OneSales-specific ops flows. |
| `scripts/check-agent-map.sh` | Extracted | Mostly unchanged; remains generic. |
| `scripts/check-agent-guides.sh` | Extracted + simplified | Kept structural checks; removed OneSales wording rules. |
| `scripts/check-rust-file-loc.sh` | Extracted | Preserved as a generic repo policy check. |
| `scripts/check-migration-immutability.sh` | Extracted | Parameterized through rendered migration path. |
| `scripts/generate-sqlx-unchecked-queries-todo.sh` | Extracted + generalized | Now scans configurable crate roots. |
| `scripts/check-sqlx-unchecked-non-test.sh` | Extracted | Kept generic. |
| `scripts/check-schema-dump.sh` | Extracted + generalized | Calls a configurable schema dump command. |
| `scripts/enforce-coverage.js` | Extracted | Kept generic. |
| `scripts/new-checkout.sh` | Extracted + generalized | Uses current repo basename instead of OneSales-specific naming. |
| `.github/workflows/agent-map-check.yml` | Templated | Runner label is configurable. |
| `.github/workflows/repo-policy.yml` | Templated subset | Keeps core policy checks. |
| `.github/workflows/rust-tests.yml` | Templated subset | Simplified to generic fmt, clippy, and locked workspace tests. |
| `.github/workflows/webapp-checks-reusable.yml` + app workflows | Consolidated | Replaced with one generated matrix-style workflow or a disabled placeholder. |
| `scripts/dev.sh` | Excluded | Too application-specific. Downstream repos implement `dev_command`. |
| `scripts/dev-bootstrap.sh` | Excluded | App-specific onboarding/bootstrap logic. |
| OneSales crate guides | Excluded | Remain project-owned because ownership and entrypoints are repo-specific. |
