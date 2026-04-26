# Repository Guidelines

This repository uses the shared `jig.sh` workflow. Keep repo-local business rules and ownership guidance in crate-level guides; keep generic agent workflow and repo policy here.

## Start Here

- Use this file for repo-wide defaults.
- Open [agent-map.md](./agent-map.md) before backend work.
- Read the nearest crate-level `AGENTS.md` before changing a crate.
- Use `.agent/PLANS.md` when writing an ExecPlan for a complex feature or refactor.
- Use `scripts/jig` for the typed repo contract and `scripts/jig mcp` for MCP clients.
- Treat `.agent/state/*.jsonl` as append-only repo memory.

## Dogfooding This Harness

This repo is both the `jig` source tree and an adopted `jig` harness repo. Prefer validating work through `scripts/jig` so changes exercise the same CLI, MCP, contract, and receipt paths that generated repos use.

When changing the `jig` runtime itself, build a dev binary and force the launcher to use it before running harness commands:

```sh
cargo build -p jig-sh --bin jig
export JIG_DEV_BIN=target/debug/jig
```

For substantial work, open a session and plan, run checks through `scripts/jig --plan-id`, then inspect receipts:

```sh
session_json="$(scripts/jig session-start)"
plan_json="$(scripts/jig plans-open --title "Describe the work" --body "Validation plan.")"
plan_id="$(printf '%s' "$plan_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["plan_id"])')"

scripts/jig contract-check --plan-id "$plan_id"
scripts/jig test --plan-id "$plan_id"
scripts/jig receipts-list --plan-id "$plan_id"
scripts/jig state-summary
```

Do not rely on the repo-local cached `jig` binary for runtime changes unless you have intentionally refreshed it. `JIG_DEV_BIN` is the expected local-development cutover.

## Compatibility And Cutovers

- Prefer direct cutovers only for internal code-only changes that can ship in one coordinated deploy.
- Preserve compatibility or stage rollouts for persisted database state, queued job types, public API contracts, bookmarked routes, webhook boundaries, or source-of-truth moves that can straddle deploys.


## Backend Defaults

- Treat `crates` as Rust crate roots.

- Keep transport logic thin and business logic in the owning crate.


## Frontend Defaults

No web apps are configured in `.jig.yml`.


## Preferred Commands

- `make bootstrap`
- `make dev`
- `make test`
- `make fmt-check`
- `make clippy`
- `make contract-check`
- `make check-agent-map`
- `make check-agent-guides`
- `make check-rust-file-loc`

- `make ci`

## Done Means

- Run the relevant local verification for the area you changed.
- For `jig` runtime changes, verify at least one representative path through `JIG_DEV_BIN=target/debug/jig scripts/jig ...` and check the resulting receipts or `state-summary`.
- For backend changes, finish with `make test`.
- Review the generated diff for stale docs, policy drift, or missing dependent updates.

## Crate Guide Requirements

Every backend crate under the configured crate roots should have an `AGENTS.md` with these sections:

- `## Purpose`
- `## Key entrypoints`
- `## Edit here for X`
- `## Invariants`
- `## Common commands`
