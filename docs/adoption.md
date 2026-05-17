# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Render the harness into that repo with `jig adopt . --rust-migration-dir migrations`. For tooling-only repos, pass `--sqlx-enabled false` instead of the migration flag. Release builds of `jig adopt` default to the official `jig-sh` template at `https://github.com/bpcakes/jig-sh.git`, pinned to the release tag for the installed Jig version. Unreleased or dirty local builds require `--template /path/to/jig-sh --template-mode committed` or an explicit `--vcs-ref` so they do not render stale release templates.
3. For local dogfooding, commit or stash template checkout changes before rendering. If you need to test in-progress template edits, make a temporary local commit and update from that committed source.
   When testing generated launchers with `JIG_DEV_BIN`, rebuild the dev binary after changing Jig and unset the variable if that binary no longer matches `.jig.toml`; generated launchers hard-fail on mismatches instead of falling back to the cache.
4. Confirm `.jig.toml` was generated with the intended profile and template source. The default points at the official portable URL; override `template_source_url` only when adopting from a local checkout, fork, or private template. If the repo already had a root `Makefile`, Jig records `makefile_enabled = false` and leaves that file project-owned. Review the remaining paths, commands, and `[dev]` proxy defaults such as `tld`, `lan`, and `workspace_discovery` before committing. Command-backed `*_command` values run through non-login `bash -c`, so put any required toolchain setup in the command string or in project-owned scripts. Jig rejects unknown `.jig.toml` keys; after upgrading an existing repo, remove or rename any unknown keys reported by `scripts/jig` before rerunning commands.
5. Review the root `AGENTS.md`. Existing repo guidance is preserved; Jig inserts or updates only the `<!-- BEGIN JIG MANAGED BLOCK -->` section.
6. Add or adapt crate-level `AGENTS.md` files for each backend crate.
7. Run `scripts/jig agent doctor --summary`. If Jig Codex skills are missing and you want this client to use them, run `scripts/jig agent bootstrap`.
8. Run the generated local checks and `scripts/jig check contract`. If web app dependencies, nested Rust projects, or other project setup must happen during bootstrap, set `bootstrap_command` explicitly; the generated default runs `cargo fetch` only when a root `Cargo.toml` exists.
9. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
10. Commit the generated files and then switch CI to use the new workflows.

Before publishing a generated repo contract or wiring long-lived MCP clients to it, review [Public Contract](./public-contract.md) for the stable CLI, MCP, and manifest guarantees.

For later template updates:

```sh
jig update
```

For remote template sources, plain `jig update` advances to the remote default branch unless you pass `--vcs-ref`. Use `jig update --recopy` when you want to re-render from the stored `_commit` instead.

If the repo was adopted from a local committed checkout, update that checkout to the desired commit and run:

```sh
jig update --template /path/to/jig-sh --template-mode committed
```

After editing `.jig.toml`, re-render the repo with:

```sh
jig update --recopy
```

`jig update` refuses to overwrite or remove changed template-managed files. Re-run with `--force` when the rendered output should replace those paths.

When updating SQLx repos that have `schema_dump_enabled = false`, remove stale `jig.schema_check` entries from `work.gates`; current templates render schema-check commands, tools, and gates only when schema dumps are enabled.

When moving a command-backed repo from contract v2 to v3, grep CI, scripts, docs, and agent instructions for old root check commands such as `scripts/jig fmt-check`, `scripts/jig contract-check`, and `scripts/jig agent-map check`; update them to `scripts/jig check ...` before relying on the new contract.

## What To Keep Project-Owned

- application code
- crate ownership boundaries
- crate-level agent guides
- root `AGENTS.md` content outside the Jig managed block
- schema dump implementation details
- app-specific dev orchestration
- any environment-specific onboarding or demo bootstrap flows

## First Validation Pass

After rendering, validate at minimum:

```sh
scripts/jig bootstrap
scripts/jig check contract
scripts/jig check fmt
scripts/jig check clippy
scripts/jig check test
```

If `sqlx_enabled` is `true`, also validate:

```sh
scripts/jig check sqlx
```

If SQLx and schema dumps are enabled:

```sh
scripts/jig check schema
```

When `makefile_enabled = true`, the generated Makefile adapter also exposes policy-helper targets such as `make check-agent-map`, `make check-agent-guides`, and `make check-rust-file-loc`.

If web apps are configured, confirm each app has the expected package scripts before enabling the web workflow.

If you want an MCP client to discover the repo automatically, point it at the generated `.mcp.json`, which launches `scripts/jig mcp`.

On a fresh machine, `scripts/jig agent doctor` is the read-only agent tooling check and exits nonzero until required setup is complete. Add `--summary` for a concise human-readable readiness report; omit it for stable JSON automation output. The top-level `ok` result requires Codex marketplace support and registered marketplace sources; plugin enablement is reported as diagnostic detail. `scripts/jig agent bootstrap` is explicit because it runs `codex plugin marketplace add` and mutates user-level Codex config. For local dogfooding with an existing sibling skills checkout, use:

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
```
