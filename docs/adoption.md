# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Render the kit into that repo with `jig adopt . --template /path/to/jig-sh --template-mode committed --rust-migration-dir migrations`. For tooling-only repos, pass `--sqlx-enabled false` instead of the migration flag.
3. For local dogfooding, commit or stash template checkout changes before rendering. If you need to test in-progress template edits, make a temporary local commit and update from that committed source.
4. Confirm `.jig.yml` was generated with the intended profile. If the repo will be shared across machines, set `template_source_url` to a portable git source, then review the remaining paths and commands before committing.
5. Add or adapt crate-level `AGENTS.md` files for each backend crate.
6. Run the generated local checks and `make contract-check`.
7. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
8. Commit the generated files and then switch CI to use the new workflows.

Before publishing a generated repo contract or wiring long-lived MCP clients to it, review [Public Contract](./public-contract.md) for the stable make-backed CLI, MCP, and manifest guarantees.

For later template updates:

```sh
jig update
```

If the repo was adopted from a local committed checkout, update that checkout to the desired commit and run:

```sh
jig update --template /path/to/jig-sh --template-mode committed
```

After editing `.jig.yml`, re-render the repo with:

```sh
jig update --recopy
```

`jig update` refuses to overwrite or remove changed template-managed files. Re-run with `--force` when the rendered output should replace those paths.

## What To Keep Project-Owned

- application code
- crate ownership boundaries
- crate-level agent guides
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
