# jig crate guide

## Purpose

`crates/jig` contains the repo-local `jig` CLI and MCP runtime used by generated repositories. It wraps the generated Makefile contract, manages append-only `.agent/state` memory, and handles template init/adopt/update flows.

## Key entrypoints

- `src/main.rs`: binary entrypoint.
- `src/lib.rs`: library entrypoint and module wiring.
- `src/cli.rs`: clap command definitions and top-level command dispatch.
- `src/runtime.rs`: make-backed tool execution and MCP tool call dispatch.
- `src/mcp.rs`: JSON-RPC/MCP stdio server.
- `src/state.rs`: sessions, plans, receipts, and decisions stored under `.agent/state`.
- `src/bootstrap.rs`: init/adopt/update command surface.
- `src/bootstrap/`: bootstrap support for Copier, git, staged renders, and template-source handling.

## Edit here for X

- Change CLI flags or subcommands: `src/cli.rs`.
- Change make-tool behavior or receipt recording around command execution: `src/runtime.rs`.
- Change MCP descriptors, schemas, or protocol handling: `src/mcp.rs`.
- Change session, plan, receipt, or decision persistence: `src/state.rs`.
- Change init/adopt/update behavior: `src/bootstrap.rs` and `src/bootstrap/`.
- Change git metadata captured in receipts: `src/git_receipts.rs`.

## Invariants

- Keep transport layers thin; shared behavior should live in runtime, state, or bootstrap helpers.
- Preserve generated-repo compatibility for `.jig.yml`, `.agent/jig-contract.json`, and `.agent/state/*.jsonl`.
- Treat `.agent/state/*.jsonl` as append-only unless a migration path is explicit.
- Keep make-backed tools aligned with the generated contract manifest and template outputs.
- Do not make template update flows switch source identity implicitly.

## Common commands

- `cargo test -p jig-sh`
- `cargo test --workspace`
- `make contract-check`
- `make check-agent-guides`
- `make check-agent-map`
