# jig-dev-proxy

`jig-dev-proxy` is an internal support crate for the matching `jig-sh` CLI
release. It is published because crates.io requires published path dependencies
for `jig-sh`, not because it is intended as a stable third-party library.

Use the `jig-sh` CLI as the public interface. This crate's Rust API and JSON
command envelopes may change between matching `jig-sh` releases.

Development app commands are trusted repo-configured commands. They intentionally
inherit the caller environment so package managers, local credentials, and dev
tooling keep working. The long-running background proxy process is different:
it starts with a constrained environment and should not be used to run arbitrary
repo commands.
