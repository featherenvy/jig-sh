# jig-typescript crate guide

## Purpose

`crates/jig-typescript` owns TypeScript feature-area contract metadata and TypeScript/frontend harness checks as they are extracted from `jig-sh`.

## Key entrypoints

- `src/lib.rs`: TypeScript command keys, required tool mapping, and availability messages.

## Edit here for X

- Add a TypeScript check exposed through Jig: `src/lib.rs`.
- Change frontend app gate requirements: `src/lib.rs`.

## Invariants

- Do not depend on `jig-sh`; keep TypeScript metadata usable by the registry.
- Keep TypeScript/frontend rules independent from Rust and SQLx rules.
- Keep package-manager process execution out of this crate until TypeScript policy extraction is explicit.

## Common commands

- `cargo test -p jig-typescript`
- `cargo test -p jig-sh`
