# jig-rust crate guide

## Purpose

`crates/jig-rust` owns Rust feature-area contract metadata and Rust-specific harness checks as they are extracted from `jig-sh`.

## Key entrypoints

- `src/lib.rs`: Rust command keys, required tool mapping, and feature descriptor.

## Edit here for X

- Add a Rust check exposed through Jig: `src/lib.rs`.
- Change Rust required tool rules: `src/lib.rs`.

## Invariants

- Do not depend on `jig-sh`; keep this crate reusable by the registry.
- Keep Rust feature rules independent from SQLx and TypeScript feature rules.
- Keep process execution out of this crate until the matching policy code is intentionally extracted.

## Common commands

- `cargo test -p jig-rust`
- `cargo test -p jig-sh`
