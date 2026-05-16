# Jig Answer File Examples

These TOML files are starting points for `jig init --answers-file` or
`jig adopt --answers-file`.

Keep these examples in lockstep with `tests/fixtures/*.toml`; release and
fixture checks verify matching contents so visible answer files and fixture
coverage do not drift.

- `tooling-only.toml`: Rust workspace with no SQLx or web app checks.
- `backend-only.toml`: SQLx-enabled Rust backend with schema dump checks disabled.
- `full-stack.toml`: SQLx-enabled backend with frontend app entries.

Copy an example, adjust repository names, paths, and commands, then run
`jig init` or `jig adopt` with `--answers-file`.
