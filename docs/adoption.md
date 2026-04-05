# Adoption Guide

## Recommended Rollout

1. Start with an existing repository that already has a stable Cargo workspace and CI.
2. Render the kit into that repo with `copier`.
3. Confirm `.agentic-kit.yaml` was generated, then review it and adjust paths or commands before committing.
4. Add or adapt crate-level `AGENTS.md` files for each backend crate.
5. Run the generated local checks.
6. Wire any missing project-owned scripts such as `scripts/dump-schema.sh` if schema dumps are enabled.
7. Commit the generated files and then switch CI to use the new workflows.

For later template updates:

```sh
uvx --from copier copier update --trust --defaults --answers-file .agentic-kit.yaml
```

After editing `.agentic-kit.yaml`, re-render the repo with:

```sh
uvx --from copier copier recopy --trust --defaults --overwrite --answers-file .agentic-kit.yaml
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
make check-agent-map
make check-agent-guides
make check-rust-file-loc
make check-sqlx-unchecked-non-test
```

If schema dumps are enabled:

```sh
make schema-check
make sqlx-check
```

If web apps are configured, confirm each app has the expected package scripts before enabling the web workflow.
