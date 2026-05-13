# Public Contract

`jig` exposes a make-backed repo contract through three surfaces:

- CLI commands from `scripts/jig`
- make-backed MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories may rely on the contract described here when they pin a `jig_version` in `.jig.toml` and keep `scripts/jig`, `.mcp.json`, and `.agent/jig-contract.json` in sync with that version.

Structured work commands and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig work ...` and `scripts/jig agent doctor`, and MCP tools named `jig.work_*` and `jig.agent_doctor`, but they are not part of contract version `1` and are not declared in `.agent/jig-contract.json`.

The structured work namespace includes native check gates. Gates are configured in `.jig.toml`, evaluated from receipts, and enforced by `scripts/jig work finish`. They remain runtime-owned because they compose stable make-backed tools with append-only work state rather than adding new make-backed contract tools.

## Contract Version

`.agent/jig-contract.json` has one schema version:

- `contract_version`: version of the generated tool manifest, make-target wiring, and make-backed command surface

Version `1` is the current stable make-backed contract. A compatible change may add optional fields, optional tools, optional make targets, or new make-backed CLI/MCP commands. A breaking change must increment `contract_version` before generated repos depend on it.

Breaking `contract_version` changes include:

- removing or renaming a stable make-backed tool
- removing or renaming a stable generated make target
- changing a stable make-backed command argument from optional to required
- changing the meaning or type of a stable make-backed JSON request or response field
- changing `.agent/jig-contract.json` in a way older runtimes cannot ignore

## Stable Manifest Fields

Generated repos and MCP clients may rely on these top-level fields in `.agent/jig-contract.json`:

- `contract_version`
- `tool_namespace`
- `jig_version`
- `required_make_targets`
- `optional_make_targets`
- `tools`

Each tool entry has these stable fields:

- `name`
- `kind`
- `description`
- `target`

For `kind: "make"` tools, `target` is either the generated make target to invoke or `null` for tools that accept a target-like argument, such as `jig.run_target`.

Consumers should ignore unknown top-level manifest fields and unknown fields inside tool entries.

## Stable Tools

The following make-backed tool names are stable in contract version `1` when declared in the manifest:

- `jig.fmt_check`
- `jig.clippy`
- `jig.test`
- `jig.test_locked`
- `jig.contract_check`
- `jig.run_target`

SQLx-specific tools are stable when `sqlx_enabled` rendered them into the manifest:

- `jig.sqlx_check`
- `jig.schema_check`
- `jig.schema_dump`
- `jig.migration_add`

A generated repo may omit optional tools that do not apply to its configuration. Clients must discover available make-backed tools from `.agent/jig-contract.json` or MCP tool listing instead of assuming SQLx or schema-dump support.

## Stable JSON Behavior

All successful stable CLI and MCP command responses are JSON objects. Stable response fields are additive: existing fields should keep their names, types, and meanings for the current contract version, and new fields may be added.

Stable common response fields:

- `ok`: boolean success indicator
- `receipt_id`: receipt identifier when the command records a receipt

Make-backed tools return:

- `tool`
- `target`
- `args`
- `result.exit_status`
- `result.stdout`
- `result.stderr`
- `receipt_id`

## Runtime State

`.agent/state/*.jsonl` is runtime-owned append-only memory. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place.

Current JSONL state files:

- `sessions.jsonl`
- `plans.jsonl`
- `receipts.jsonl`
- `decisions.jsonl`

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records.

The active-session pointer is cache state, currently resolved through git as `jig-current-session.txt` and falling back under `.agent/.cache/`. Generated repos should not treat that path as a durable JSONL record.

Structured work commands use the `jig.work_*` CLI and MCP namespace, but state-operation receipts keep their historical tool names for compatibility with existing receipt history and filters:

- `jig.session_start`
- `jig.session_end`
- `jig.plans_open`
- `jig.plans_append`
- `jig.plans_close`
- `jig.decisions_add`

## Work Gates

`work.gates` in `.jig.toml` declares required evidence before structured work can finish. `kind: check` gates reference make-backed tools from `.agent/jig-contract.json`; `scripts/jig work check --plan-id ...` runs them and records normal receipts. `scripts/jig work gates --plan-id ...` reports gate status from the latest fresh receipt for each gate tool on that plan.

`scripts/jig work finish --plan-id ...` fails when any required gate is missing, failed, stale, unknown, or unsupported. Older `work.checks` entries are still accepted for compatibility and backfill missing required check gates during migration. If the same tool is declared in `work.gates`, that explicit gate entry is authoritative.

Fresh check evidence means the non-`.agent/` worktree fingerprint did not change while `work check` ran and still matches the current worktree. Generated outputs should therefore be committed, ignored, or settled before required gates are used as finish evidence. If a check creates expected files, review those files and rerun `work check` to record fresh evidence.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint`, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older receipts deserialize, but their gate freshness is `unknown`.

Non-`check` gate kinds are reserved for future structured integrations such as Codex review gates. They are parsed and reported as unsupported until the runtime can record and validate machine-readable review evidence.

`work.refinements` is reserved for future refinement execution and is rejected with a configuration error until support exists.

## Rollout Rules

Use this sequence for public contract changes:

1. Add the new field, tool, or command in a backward-compatible way.
2. Update `.agent/jig-contract.json.jinja`, runtime dispatch, MCP exposure, and docs in the same change.
3. Keep old fields and commands working for the current contract version.
4. Run `make release-check` before release.
5. Only remove or redefine stable behavior after incrementing `contract_version`.

Generated repos can rely on:

- `scripts/jig` enforcing the exact `jig_version` from `.jig.toml`
- `make contract-check` detecting missing generated runtime wiring
- stable make target names listed in `required_make_targets`
- make-backed tool availability being discoverable from `.agent/jig-contract.json` and MCP
- state files being runtime-owned append-only records

Generated repos should not rely on:

- private Rust module layout inside `crates/jig`
- unlisted make targets
- undocumented JSON fields
- physical ordering of fields in JSON objects
- SQLx or schema-dump tools unless present in the manifest
- versioned state-file schemas under `.agent/state/*.jsonl`
