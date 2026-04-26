# Public Contract

`jig` exposes one repo contract through three surfaces:

- CLI commands from `scripts/jig`
- MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories may rely on the contract described here when they pin a `jig_version` in `.jig.yml` and keep `scripts/jig`, `.mcp.json`, and `.agent/jig-contract.json` in sync with that version.

## Contract Versions

`.agent/jig-contract.json` has two independent schema versions:

- `contract_version`: version of the tool manifest, make-target wiring, and command surface
- `memory_schema_version`: version of files under `.agent/state/*.jsonl`

Version `1` is the current stable contract. A compatible change may add optional fields, optional tools, optional make targets, or new CLI/MCP commands. A breaking change must increment the relevant version before generated repos depend on it.

Breaking `contract_version` changes include:

- removing or renaming a stable tool
- removing or renaming a stable generated make target
- changing a stable command argument from optional to required
- changing the meaning or type of a stable JSON request or response field
- changing `.agent/jig-contract.json` in a way older runtimes cannot ignore

Breaking `memory_schema_version` changes include:

- removing or renaming fields in existing `.agent/state/*.jsonl` records
- changing the type or meaning of existing state fields
- requiring old state records to be rewritten before reads succeed

Additive state changes should use optional fields with tolerant readers. Readers must continue to accept older records that lack newly added fields.

## Stable Manifest Fields

Generated repos and MCP clients may rely on these top-level fields in `.agent/jig-contract.json`:

- `contract_version`
- `memory_schema_version`
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

For `kind: "memory"` tools, `target` is `null` and the runtime handles the operation without shelling out to `make`.

Consumers should ignore unknown top-level manifest fields and unknown fields inside tool entries.

## Stable Tools

The following tool names are stable in contract version 1 when declared in the manifest:

- `jig.fmt_check`
- `jig.clippy`
- `jig.test`
- `jig.test_locked`
- `jig.contract_check`
- `jig.run_target`
- `jig.session_start`
- `jig.session_end`
- `jig.plans_open`
- `jig.plans_append`
- `jig.plans_close`
- `jig.receipts_list`
- `jig.state_summary`
- `jig.decisions_add`

SQLx-specific tools are stable when `sqlx_enabled` rendered them into the manifest:

- `jig.sqlx_check`
- `jig.schema_check`
- `jig.schema_dump`
- `jig.migration_add`

A generated repo may omit optional tools that do not apply to its configuration. Clients must discover available tools from `.agent/jig-contract.json` or MCP tool listing instead of assuming SQLx or schema-dump support.

## Stable JSON Behavior

All successful CLI and MCP command responses are JSON objects. Stable response fields are additive: existing fields should keep their names, types, and meanings for the current contract version, and new fields may be added.

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

Memory tools return operation-specific identifiers and state data. Current stable identifiers include:

- `session_id` for session commands
- `plan_id` for plan commands
- `decision_id` for decision commands
- `receipts` for `jig.receipts_list`
- `counts`, `open_plans`, `recent_receipts`, and `recent_decisions` for `jig.state_summary`

`jig.receipts_list` supports optional `session_id`, `plan_id`, `tool_name`, `failed_only`, and `limit` filters. Receipt list entries include the persisted receipt fields plus an additive `diff_summary` presentation field.

`jig.receipts_list` and `jig.state_summary` are read-only and do not create receipts for the query operation.

## State Files

`.agent/state/*.jsonl` is runtime-owned append-only memory. Generated repos may back up, inspect, or remove these files intentionally, but application code should not edit individual records in place.

Current JSONL state files:

- `sessions.jsonl`
- `plans.jsonl`
- `receipts.jsonl`
- `decisions.jsonl`

State readers should tolerate missing files by treating them as empty. JSONL readers should ignore blank lines and fail loudly on malformed nonblank records.

The active-session pointer is cache state, currently resolved through git as `jig-current-session.txt` and falling back under `.agent/.cache/`. Generated repos should not treat that path as a durable JSONL record.

## Rollout Rules

Use this sequence for public contract changes:

1. Add the new field, tool, or command in a backward-compatible way.
2. Update `.agent/jig-contract.json.jinja`, runtime dispatch, MCP exposure, and docs in the same change.
3. Keep old fields and commands working for the current contract version.
4. Run `make contract-check` and fixture validation before release.
5. Only remove or redefine stable behavior after incrementing `contract_version` or `memory_schema_version`.

Generated repos can rely on:

- `scripts/jig` enforcing the exact `jig_version` from `.jig.yml`
- `make contract-check` detecting missing generated runtime wiring
- stable make target names listed in `required_make_targets`
- tool availability being discoverable from `.agent/jig-contract.json` and MCP
- state files remaining append-only within a memory schema version

Generated repos should not rely on:

- private Rust module layout inside `crates/jig`
- unlisted make targets
- undocumented JSON fields
- physical ordering of fields in JSON objects
- SQLx or schema-dump tools unless present in the manifest
