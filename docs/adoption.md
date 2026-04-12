# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Render the kit into that repo with `jig adopt --template /path/to/jig-sh --template-mode committed .`. For tooling-only repos, pass `--sqlx-enabled false` during the initial adopt instead of rendering SQLx files first and recopying them away.
3. For local dogfooding against an in-progress template checkout, use `--template-mode working-tree` instead. That mode snapshots the local template checkout into `.agent/.cache/template-source` so later `jig update` runs stay local-first without manual tarballs or ad hoc snapshot repos.
4. Confirm `.jig.yml` was generated with the intended profile. If the repo will be shared across machines, set `template_source_url` to a git source where `refs/heads/<default_branch>` already contains the generated `_commit`, then review the remaining paths and commands before committing.
5. Add or adapt crate-level `AGENTS.md` files for each backend crate.
6. Run the generated local checks and `make contract-check`.
7. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
8. Commit the generated files and then switch CI to use the new workflows.

For later template updates:

```sh
jig update
```

If the repo was adopted from a local working tree, `jig update` refreshes the stored local snapshot automatically.

To relink a local-first repo onto a clean committed checkout, run:

```sh
jig update --template /path/to/jig-sh --template-mode committed
```

After editing `.jig.yml`, re-render the repo with:

```sh
jig update --recopy
```

`jig` uses Copier under the hood. If you need the raw equivalent commands:

```sh
uvx --from copier copier update --trust --answers-file .jig.yml
uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .jig.yml
```

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
