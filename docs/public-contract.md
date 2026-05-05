# Public Contract

`jig` exposes a make-backed repo contract through three surfaces:

- CLI commands from `scripts/jig`
- make-backed MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories may rely on the contract described here when they pin a `jig_version` in `.jig.yml` and keep `scripts/jig`, `.mcp.json`, and `.agent/jig-contract.json` in sync with that version.

Session, plan, receipt, decision, and state-summary commands are runtime-owned conveniences. They may be available through the CLI and MCP server, but they are not part of contract version `1` and are not declared in `.agent/jig-contract.json`.

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

## Rollout Rules

Use this sequence for public contract changes:

1. Add the new field, tool, or command in a backward-compatible way.
2. Update `.agent/jig-contract.json.jinja`, runtime dispatch, MCP exposure, and docs in the same change.
3. Keep old fields and commands working for the current contract version.
4. Run `make contract-check` and fixture validation before release.
5. Only remove or redefine stable behavior after incrementing `contract_version`.

Generated repos can rely on:

- `scripts/jig` enforcing the exact `jig_version` from `.jig.yml`
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
