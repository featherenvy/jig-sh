# jig-core crate guide

## Purpose

`crates/jig-core` owns the base Jig harness feature metadata: core command keys, core native tool declarations, and core required-tool rules.

## Key entrypoints

- `src/lib.rs`: core feature descriptor and base harness tool requirements.

## Edit here for X

- Add or change a base harness command key: `src/lib.rs`.
- Change core required tool rules: `src/lib.rs`.
- Change core native tool metadata: `src/lib.rs`.

## Invariants

- Keep this crate narrowly scoped to base harness feature metadata.
- Do not add runtime orchestration, state handling, MCP transport, bootstrap implementation, or process execution here.
- Depend only downward on `jig-contract`; aggregation belongs in `jig-features`.

## Common commands

- `cargo test -p jig-core`
- `cargo test -p jig-features`
- `cargo test -p jig-sh`
