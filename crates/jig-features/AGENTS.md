# jig-features crate guide

## Purpose

`crates/jig-features` aggregates feature-area crates into the runtime registry used for contract validation, tool metadata, and availability messages.

## Key entrypoints

- `src/lib.rs`: registry aggregation and lookup helpers.

## Edit here for X

- Register a new feature-area crate: `src/lib.rs`.
- Change core harness tool requirements: `../jig-core/src/lib.rs`.
- Change registry lookup semantics: `src/lib.rs`.

## Invariants

- Keep this crate metadata-focused; do not execute native tools here.
- Depend on feature-area crates and `jig-contract`, not on `jig-sh`.
- Keep feature order non-contractual; public set-like outputs must sort and dedup.

## Common commands

- `cargo test -p jig-features`
- `cargo test -p jig-sh`
