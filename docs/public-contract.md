# Public Contract

`jig` exposes a repo command contract through three surfaces:

- CLI commands from `scripts/jig`
- MCP tools from `scripts/jig mcp`
- `.agent/jig-contract.json`

Generated repositories may rely on the contract described here when they pin a `jig_version` in `.jig.toml` and keep `scripts/jig`, `.mcp.json`, and `.agent/jig-contract.json` in sync with that version.

Structured work commands and agent tooling checks are runtime-owned conveniences. They are available through commands such as `scripts/jig work ...` and `scripts/jig agent doctor`, and MCP tools named `jig.work_*` and `jig.agent_doctor`, but they are not part of the generated command contract and are not declared in `.agent/jig-contract.json`.

Some runtime-owned CLI commands expose explicit human-output flags, such as `scripts/jig agent doctor --summary`, `scripts/jig work status --summary`, and `scripts/jig work start --print-plan-id`. These outputs are for terminal scanning or shell integration and are not stable machine-readable contract output; automation should use the default JSON output or MCP tools.

The structured work namespace includes native check gates. Gates are configured in `.jig.toml`, evaluated from receipts, and enforced by `scripts/jig work finish`. They remain runtime-owned because they compose stable execution tools with append-only work state rather than adding new generated contract tools.

Local development proxy commands are also runtime-owned. `scripts/jig dev` and `scripts/jig proxy ...` manage machine-local processes, ports, routes, certificates, and optional user services. They are configured from `.jig.toml` but are intentionally absent from `.agent/jig-contract.json` because they do not represent repository checks.

Runtime-owned local development commands include `dev`, `proxy start`, `proxy stop`, `proxy list`, `proxy prune`, `proxy run`, `proxy alias`, `proxy cert generate`, `proxy cert status`, `proxy cert trust --accept-trust-scope`, `proxy cert untrust --accept-trust-scope`, `proxy service install --accept-service-scope`, `proxy service status`, and `proxy service uninstall`. Builds made with `--no-default-features` keep the contract, MCP, and work-receipt runtime but return clear errors for `dev` and `proxy`; use that build mode for MCP/contract-only consumers that do not need the TLS/HTTP dev-proxy stack.

LAN mode exposes the Jig proxy listener to the local network, not child app listeners directly. Process routes may be reached from other devices only through the proxy, with the original routed hostname in DNS, a hosts file, or the HTTP `Host` header. Alias routes stay loopback-client-only so LAN clients cannot use Jig as an open forward proxy.

The `tool_defs::cli_command` names for these runtime-owned commands are parser labels only. They do not add generated tools to `.agent/jig-contract.json` and do not expose MCP tools for proxy process or service management.

Because the local development proxy is runtime-owned, its JSON response fields, machine-local state layout under `JIG_PROXY_STATE_DIR` or `~/.jig/proxy`, service-file contents, certificate files, route hostname format, and nonzero error exit statuses are not part of `.agent/jig-contract.json`. Generated repos should pin `jig_version` for proxy behavior and treat those details as same-version runtime behavior rather than as public contract fields.

The current explicit acknowledgement flags, including `--accept-trust-scope` and `--accept-service-scope`, are runtime safety gates rather than generated contract fields. Automation should keep using the pinned `jig_version` CLI help and behavior instead of assuming those opt-in prompts are stable across runtime upgrades.

Runtime-owned `.jig.toml` sections are intentionally strict: unknown keys are rejected so local typos fail fast. New keys in `[work]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, or app tables require a Jig runtime/template update and a documented migration note; they do not require a `.agent/jig-contract.json` version bump unless they also change generated CLI or MCP contract behavior.

## Contract Version

`.agent/jig-contract.json` has these schema versions:

- `contract_version`: version of the generated tool manifest and command surface

Version `1` is the legacy make-backed contract. Version `2` is the legacy root-check command-backed contract. Version `3` is the current command-backed contract with checks grouped under `scripts/jig check ...`. Moving a repo from version `2` to `3` requires updating CI, scripts, docs, and agent instructions that invoke the old top-level check commands. A compatible change may add optional fields, optional tools, optional commands, optional make targets, or new CLI/MCP commands. A breaking change must increment `contract_version` before generated repos depend on it.

Breaking `contract_version` changes include:

- removing or renaming a stable generated tool
- removing or renaming a stable generated command key
- changing a stable command argument from optional to required
- changing the meaning or type of a stable JSON request or response field
- changing `.agent/jig-contract.json` in a way older runtimes cannot ignore

## Stable Manifest Fields

Generated repos and MCP clients may rely on these top-level fields in `.agent/jig-contract.json`:

- `contract_version`
- `tool_namespace`
- `jig_version`
- `required_commands` for command-backed contract versions `2` and `3`
- `required_make_targets` and `optional_make_targets` for legacy contract version `1`
- `tools`

Each tool entry has these stable fields:

- `name`
- `kind`
- `description`
- `command` for `kind: "command"` tools
- `target` for `kind: "make"` tools

For `kind: "command"` tools, `command` is the top-level `.jig.toml` command key the runtime executes from the repo root. For `kind: "make"` tools, `target` is either the generated make target to invoke or `null` for tools that accept a target-like argument, such as `jig.run_target`.

Command-backed contract versions intentionally have no `optional_commands` field. A command-backed tool is valid only when its command key is listed in `required_commands`; optional capability is represented by omitting the tool entirely when the rendered repo profile does not support it.

Consumers should ignore unknown top-level manifest fields and unknown fields inside tool entries.

## Stable Tools

The following tool names are stable in command-backed contract versions when declared in the manifest:

- `jig.bootstrap`
- `jig.fmt_check`
- `jig.clippy`
- `jig.test`
- `jig.test_locked`
- `jig.contract_check`

SQLx-specific tools are stable when the rendered repo profile includes them:

- `jig.sqlx_check`
- `jig.migration_add`
- `jig.schema_check` when schema dumps are enabled
- `jig.schema_dump` when schema dumps are enabled

Contract version `1` exposed these legacy make-backed tool names:

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

A generated repo may omit optional tools that do not apply to its configuration. Clients must discover available tools from `.agent/jig-contract.json` or MCP tool listing instead of assuming SQLx or schema-dump support.

## Stable JSON Behavior

All successful stable CLI and MCP command responses are JSON objects unless a runtime-owned command explicitly documents a human-output flag. Stable response fields are additive: existing fields should keep their names, types, and meanings for the current contract version, and new fields may be added.

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

Command-backed tools return the same common fields plus `command_key`, which identifies the `.jig.toml` command key that was executed.

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

`work.gates` in `.jig.toml` declares required evidence before structured work can finish. `kind: check` gates reference execution tools from `.agent/jig-contract.json`; `scripts/jig work check --plan-id ...` runs them and records normal receipts. `scripts/jig work gates --plan-id ...` reports gate status from the latest fresh receipt for each gate tool on that plan.

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
4. Run the configured release checks before release.
5. Only remove or redefine stable behavior after incrementing `contract_version`.

Generated repos can rely on:

- `scripts/jig` enforcing the exact `jig_version` from `.jig.toml`
- `scripts/jig check contract` detecting missing generated runtime wiring
- stable command keys listed in `required_commands` for command-backed contract versions
- tool availability being discoverable from `.agent/jig-contract.json` and MCP
- state files being runtime-owned append-only records

Generated repos should not rely on:

- private Rust module layout inside `crates/jig`
- unlisted Make targets or project scripts
- undocumented JSON fields
- physical ordering of fields in JSON objects
- SQLx or schema-dump tools unless present in the manifest
- versioned state-file schemas under `.agent/state/*.jsonl`
