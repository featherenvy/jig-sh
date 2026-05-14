# Contributing to jig.sh

## Local development

Default `jig init` and `jig adopt` use the official remote template at the `vVERSION` tag for the running binary. When testing unreleased local changes with `cargo run -p jig-sh -- adopt ...`, pass `--template /path/to/jig-sh --template-mode committed` to render from your checkout, or pass `--vcs-ref main` to use the current official branch.

During a release, the remote `vVERSION` tag is pushed after the crates publish step succeeds. If you install a freshly published binary before the tag is visible on GitHub, use `--vcs-ref main` or a local `--template` path for the first render, then retry the pinned default after the tag is pushed.

## Release

Use the GitHub Actions `Release` workflow for the lowest-touch release path. Leave `version` blank to publish the next patch version, or set it explicitly. The workflow prepares the release commit, updates `CHANGELOG.md`, creates a local tag, publishes `jig-dev-proxy` and then `jig-sh` to crates.io through trusted publishing, pushes the tag to origin after both crates publish, and creates the GitHub Release.

`CHANGELOG.md` release sections are generated from git history. Conventional commit prefixes (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `perf:`, `build:`, `ci:`, `chore:`) drive the release-note categories; unprefixed commits land in `Other`. Do not hand-edit an upcoming version section before running the workflow.

### Local release steps

The local release script is the typed entrypoint for validation and manual recovery. The `release-github` target requires the GitHub CLI (`gh`) with permission to create releases.

```sh
make release-prepare RELEASE_VERSION=0.1.1
ALLOW_DIRTY=1 make release-check RELEASE_VERSION=0.1.1
make release-stage
git commit -m "Release v0.1.1"
make release-check RELEASE_VERSION=0.1.1
make release-tag RELEASE_VERSION=0.1.1
make release-publish RELEASE_VERSION=0.1.1
make release-github RELEASE_VERSION=0.1.1
```

- `release-prepare` — updates all pinned version files and regenerates `CHANGELOG.md`
- `release-check` — requires a clean worktree, verifies version wiring and changelog coverage, runs `make ci`, validates rendered fixtures, and runs crates.io publish dry runs
- `release-tag` — creates the annotated local `vVERSION` tag after the same checks
- `release-publish` — requires the tag to point at `HEAD`, publishes `jig-dev-proxy`, waits for crates.io to see it, publishes `jig-sh`, then pushes the tag to origin
- `release-github` — creates the GitHub Release from the matching `CHANGELOG.md` section

### crates.io trusted publishing setup

Before the first split-crate release, pre-create crates.io Trusted Publishing configuration for both packages (`jig-dev-proxy` and `jig-sh`), repository `bpcakes/jig-sh`, workflow `release.yml`, and environment `crates-io`. Protect that GitHub environment with required reviewers.

`release-publish` skips package versions already present on crates.io and pushes the remote tag only after every crate is published. If only part of the crate set was published, keep the same version for remaining packages; bump only when a published crate version itself must change, since crates.io versions cannot be overwritten after yank.

If a workflow run pushes the release commit but fails before the tag is pushed, rerun the workflow with the explicit prepared version instead of leaving `version` blank.

For the already-published `v0.1.0`, run the workflow with `backfill_v0_1_0=true` to create the missing GitHub Release without publishing or retagging.
