# Repo Intent For Agents

This document captures what the current codebase makes clear about `jig.sh`, plus the intent that can be reasonably inferred from its structure. It is written for agents and maintainers who need to understand the idea behind the repo before changing it.

## High-Level Intent

`jig.sh` is a reusable harness for making Rust application repositories operable by coding agents.

The core idea is to turn a repository into an operating environment for coding agents through a small, stable, repo-local contract:

- repo-wide and crate-level agent guidance
- a predictable top-level `make` interface
- a typed `scripts/jig` CLI over that interface
- an MCP server exposing the same tools to MCP clients
- append-only session, plan, receipt, and decision state under `.agent/`
- required work gates evaluated from plan-linked receipts
- native agent tooling checks for client-side Jig skills
- policy scripts and CI workflows that keep the generated contract honest

In short: `jig.sh` is a harness for making agentic software work repeatable, inspectable, and reviewable.

## What Is Certain

The README says this repo is a "Reusable harness for making Rust application repos operable by coding agents, including SQLx/Postgres backends and tooling-only Rust repos, with optional web apps."

The harness was extracted from the durable parts of an existing application workflow. The extracted pieces are generic agent guidance, a stable `make` contract, repo policy scripts, GitHub Actions workflows, a template sync flow, and the Rust `jig` runtime.

Generated or adopted repos receive assets such as `.jig.yml`, `.mcp.json`, `AGENTS.md`, `agent-map.md`, `.agent/PLANS.md`, `.agent/jig-contract.json`, `Makefile`, scripts, and workflows.

Generated repos keep `make` as the execution backend. `scripts/jig` is a typed launcher/runtime over the make-backed contract, and `scripts/jig mcp` exposes the contract to MCP clients.

The runtime is implemented in `crates/jig`. Its main responsibilities are:

- bootstrap flows: `jig init`, `jig adopt`, and `jig update`
- make-backed tool execution
- MCP protocol handling over stdio
- append-only runtime state for sessions, plans, receipts, and decisions
- agent tooling doctor/bootstrap commands for Codex-side Jig skills
- receipt metadata collection, including git changed paths and diff stats

The stable generated contract is `.agent/jig-contract.json` with `contract_version: 1`. That contract is intentionally limited to make-backed tools such as `jig.fmt_check`, `jig.clippy`, `jig.test`, `jig.test_locked`, `jig.contract_check`, `jig.run_target`, and optional SQLx/schema/migration tools.

Runtime memory tools are intentionally not part of `.agent/jig-contract.json`. They are runtime-owned conveniences exposed by the CLI and MCP server.

The root `AGENTS.md` is block-managed during adoption and update. Existing repo-specific content outside the Jig managed block is preserved.

Crate-level `AGENTS.md` files are project-owned. `jig.sh` validates their presence and required sections, but it does not generate business-specific crate ownership guidance.

The template deliberately avoids generating application code, crate guides, or schema dump implementations. Those remain owned by the consumer repository.

The repo dogfoods itself. This checkout is both the `jig` source tree and an adopted `jig` harness repo, and the root `AGENTS.md` instructs agents to validate runtime changes by building a dev binary and running through `scripts/jig` with `JIG_DEV_BIN`.

## The Product Idea

The repo appears to be designing for a future where agents work repeatedly inside long-lived repositories and need more than a README. They need:

- a map of where repo guidance lives
- a clear command contract
- machine-readable tool definitions
- a way to run checks without guessing project conventions
- required gates that stop work from finishing without evidence
- durable traces of plans, decisions, and command results
- compatibility rules so generated tooling can evolve without surprising downstream repos

The strongest product thesis visible in the code is:

> Agents should not infer how to operate a repo from scattered conventions. Repos should expose an explicit, typed, versioned harness that agents can use safely.

A shorter product phrasing is:

> Jig turns a repo into an operating environment for coding agents.

## Architecture In One Pass

`templates/project/` is the source for generated repository assets. It renders `.jig.yml`, `AGENTS.md`, `.agent/jig-contract.json`, the `Makefile`, scripts, workflows, and agent support files.

`.jig.yml` is both public configuration and the renderer answer file. It records repo settings such as `repo_name`, `default_branch`, `jig_version`, crate roots, SQLx settings, web app settings, and template source metadata.

`crates/jig/src/bootstrap.rs` and its submodules implement template application:

- `init` renders the harness into a new destination and initializes git.
- `adopt` renders the harness into an existing repo while preserving repo-owned root `AGENTS.md` content.
- `update` re-renders managed paths from stored template metadata and refuses to overwrite changed managed files unless forced.

`crates/jig/src/runtime.rs` dispatches CLI and MCP tool calls. For make-backed tools, it resolves the tool from `.agent/jig-contract.json`, runs the matching `make` target, records a receipt, and returns structured JSON.

`crates/jig/src/mcp.rs` is a minimal MCP stdio server. It lists make-backed tools from the manifest and runtime memory tools from code, then dispatches `tools/call` through the same runtime path as the CLI.

`crates/jig/src/state/` stores append-only JSONL records:

- `sessions.jsonl`: session start/end events and summaries
- `plans.jsonl`: plan open/append/close events
- `receipts.jsonl`: tool execution evidence
- `decisions.jsonl`: structured decision records

The current session pointer is cache state, not part of the durable JSONL record model.

## Design Principles Visible In The Code

**Agent-first discoverability.** `agent-map.md`, root `AGENTS.md`, crate `AGENTS.md`, MCP tool descriptors, and `.agent/jig-contract.json` all reduce the need for an agent to guess where to start.

**Make remains the portable backend.** The generated `Makefile` is the stable human- and CI-friendly execution layer. The Rust runtime wraps it rather than replacing it.

**Typed surfaces over shell conventions.** `scripts/jig` returns JSON, validates tool names against a manifest, records receipts, and exposes MCP schemas. Agents get structured results instead of scraping terminal output.

**Compatibility is explicit.** Public make-backed tools are governed by `contract_version`. Breaking changes require a contract version bump, and downstream clients are expected to discover available tools instead of assuming optional SQLx/schema support.

**Runtime memory is append-only.** State files are written as JSONL, readers tolerate missing files, and docs say application code should not edit records in place.

**Repo-specific ownership stays local.** The harness provides workflow and policy defaults, while application code, business rules, crate ownership, and schema dump details stay with the downstream repo.

**Template updates should be conservative.** Update flows preserve root `AGENTS.md` custom content, avoid implicit template source switching, and refuse to overwrite changed managed files without `--force`.

**Dogfooding matters.** Runtime changes are expected to be validated through the same `scripts/jig` launcher, MCP contract, and receipt paths generated repos use.

**Rust-first, but not only backend code.** The current scope is Cargo workspaces, Rust checks, optional SQLx/Postgres behavior, and optional Bun-based web apps.

## What This Repo Is Not Trying To Be

It is not an application framework. It does not generate app code or domain models.

It is not a replacement for `make`. It standardizes and wraps make targets.

It is not a global agent memory system. The state is repo-local and runtime-owned.

It is not trying to centralize business ownership. Crate-level guidance and application-specific commands stay project-owned.

It is not currently a general polyglot harness. The generated defaults assume Cargo workspaces, Rust formatting/clippy/tests, optional SQLx, and Bun for configured web apps.

## How Agents Should Approach This Repo

Start with root `AGENTS.md`, then `agent-map.md`, then the nearest crate guide.

For runtime changes, read `crates/jig/AGENTS.md` and use its entrypoint map:

- CLI shape: `crates/jig/src/cli.rs`
- make/MCP dispatch: `crates/jig/src/runtime.rs`
- MCP protocol: `crates/jig/src/mcp.rs`
- sessions/plans/receipts/decisions: `crates/jig/src/state.rs` and `crates/jig/src/state/`
- bootstrap and template rendering: `crates/jig/src/bootstrap.rs` and `crates/jig/src/bootstrap/`
- generated outputs: `templates/project/`

When changing the public contract, update the manifest template, runtime dispatch, MCP exposure, generated scripts/docs, and tests together.

When changing generated repo behavior, validate with fixture rendering. The broad fixture check is:

```sh
./scripts/validate-fixtures.sh
```

When changing runtime behavior, build a dev binary and dogfood through the launcher:

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work status
```
