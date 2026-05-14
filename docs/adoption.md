# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Pick a template source. Jig currently ships one general-purpose harness template, located at `templates/project` in the `jig-sh` repo. Use `--template /path/to/jig-sh` when dogfooding a local checkout, or `--template https://github.com/bpcakes/jig-sh.git` for the public source.
3. Render the harness into that repo with `jig adopt . --template /path/to/jig-sh --template-mode committed --rust-migration-dir migrations`. For tooling-only repos, pass `--sqlx-enabled false` instead of the migration flag.
4. For local dogfooding, commit or stash template checkout changes before rendering. If you need to test in-progress template edits, make a temporary local commit and update from that committed source.
   When testing generated launchers with `JIG_DEV_BIN`, rebuild the dev binary after changing Jig and unset the variable if that binary no longer matches `.jig.toml`; generated launchers hard-fail on mismatches instead of falling back to the cache.
5. Confirm `.jig.toml` was generated with the intended profile. If the repo will be shared across machines, set `template_source_url` to a portable git source, then review the remaining paths, commands, and `[dev]` proxy defaults such as `tld`, `lan`, and `workspace_discovery` before committing. Jig rejects unknown `.jig.toml` keys; after upgrading an existing repo, remove or rename any unknown keys reported by `scripts/jig` before rerunning commands.
6. Review the root `AGENTS.md`. Existing repo guidance is preserved; Jig inserts or updates only the `<!-- BEGIN JIG MANAGED BLOCK -->` section.
7. Add or adapt crate-level `AGENTS.md` files for each backend crate.
8. Run `scripts/jig agent doctor`. If Jig Codex skills are missing and you want this client to use them, run `scripts/jig agent bootstrap`.
9. Run the generated local checks and `make contract-check`.
10. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
11. Commit the generated files and then switch CI to use the new workflows.

Before publishing a generated repo contract or wiring long-lived MCP clients to it, review [Public Contract](./public-contract.md) for the stable make-backed CLI, MCP, and manifest guarantees.

For later template updates:

```sh
jig update
```

If the repo was adopted from a local committed checkout, update that checkout to the desired commit and run:

```sh
jig update --template /path/to/jig-sh --template-mode committed
```

After editing `.jig.toml`, re-render the repo with:

```sh
jig update --recopy
```

`jig update` refuses to overwrite or remove changed template-managed files. Re-run with `--force` when the rendered output should replace those paths.

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
make help
make contract-check
make check-agent-map
make check-agent-guides
make check-rust-file-loc
```

If `sqlx_enabled` is `true`, also validate:

```sh
make check-sqlx-unchecked-non-test
make sqlx-check
```

If SQLx and schema dumps are enabled:

```sh
make schema-check
```

If web apps are configured, confirm each app has the expected package scripts before enabling the web workflow.

If you want an MCP client to discover the repo automatically, point it at the generated `.mcp.json`, which launches `scripts/jig mcp`.

On a fresh machine, `scripts/jig agent doctor` is the read-only agent tooling check. Its top-level `ok` result requires Codex marketplace support and registered marketplace sources; plugin enablement is reported as diagnostic detail. `scripts/jig agent bootstrap` is explicit because it runs `codex plugin marketplace add` and mutates user-level Codex config. For local dogfooding with an existing sibling skills checkout, use:

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
```
