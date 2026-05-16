# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

Jig turns a Rust application repo into an operating environment for coding agents. Without it, agents lose context across machines, lack a stable execution contract, and leave no inspectable record of their work. Jig fixes that by generating the scaffolding once and keeping it in sync.

It makes agentic software work repeatable, inspectable, and reviewable by generating:

- **Agent context files** (`AGENTS.md`, `agent-map.md`) so coding agents know the repo layout and conventions without reading source
- **A typed `jig` runtime contract** so every machine, CI run, and agent executes the same configured commands and leaves append-only receipts under `.agent/state/`
- **An optional `Makefile` adapter** for repos that want familiar make targets without making Make the agent-facing contract
- **A local dev proxy** so app hostnames stay stable across port changes and machine restarts
- **Work gates backed by receipts** so a task cannot be marked done without a verifiable output artifact
- **Repo policy scripts and CI workflows** so linting, tests, and coverage enforcement run consistently from day one
- **Template sync via `jig update`** so the harness stays current as `jig-sh` evolves — without overwriting files you have customized

## How It Works

Jig's template lives in the `jig-sh` repository. Running `jig init` or `jig adopt` renders it into your project, producing `scripts/jig`, agent context files, CI workflows, and MCP configuration. New repos also get a Makefile adapter by default; adoption skips that adapter when the destination already has a root `Makefile`. After that first render, `jig update` keeps managed files current as the template evolves — files you have customized are never overwritten without `--force`.

## Quick Start

**Prerequisites:** Rust 1.85+, [Bun](https://bun.sh) (web targets), Postgres (when `sqlx_enabled = true`)

```sh
cargo install jig-sh
```

Generated repos install and pin their own `jig` version automatically via `scripts/install-jig.sh`. You only need a global install to run `jig init` or `jig adopt` on a repo for the first time. Help requests reuse an existing matching repo-local binary when one is available; on a cold checkout the launcher prints an explicit first-run install message before preparing the runtime.

By default, release builds of `jig init` and `jig adopt` clone the official template from GitHub at the matching `vVERSION` tag. For offline use or local head dogfooding, pass `--template /path/to/jig-sh --template-mode committed`.

**Bootstrap a new repo:**

```sh
jig init /path/to/target-repo \
  --repo-name target-repo \
  --rust-migration-dir migrations
```

For a tooling-only repo with no SQLx or migrations:

```sh
jig init /path/to/target-repo \
  --repo-name target-repo \
  --sqlx-enabled false
```

**Adopt the template in an existing repository:**

```sh
cd /path/to/target-repo
jig adopt . \
  --repo-name target-repo \
  --rust-migration-dir migrations
```

For a tooling-only repo, replace the migration flag with `--sqlx-enabled false`.

If the destination already has a root `Makefile`, `jig adopt` keeps it project-owned and records `makefile_enabled = false` in `.jig.toml`. Pass `--makefile-enabled true` only when you intentionally want Jig to manage the root Makefile adapter.

**Update an adopted repo:**

`jig update` refuses to overwrite changed template-managed files unless `--force` is passed:

```sh
cd /path/to/target-repo
jig update
```

For repos adopted from a local committed template checkout, update that checkout to the desired commit and run:

```sh
cd /path/to/target-repo
jig update \
  --template /path/to/jig-sh \
  --template-mode committed \
  --force
```

To re-render from stored `.jig.toml` answers without advancing the template source:

```sh
cd /path/to/target-repo
jig update --recopy
```

Pass `--force` if the rendered output should replace existing template-managed files.

`.jig.toml` serves as both the public repo-facing config and the renderer answers file used by `jig update --recopy`. `jig update --recopy` re-renders from the stored `_commit`; plain `jig update` advances to the current resolved template source, which means a remote template source advances to its default branch unless you pass `--vcs-ref`. The default template source is already portable; set `template_source_url` only when adopting from a local checkout, fork, or private template that should be resolved through a different canonical URL.

For repos adopted from the official default template, this means the first render is pinned to the installed Jig version, while plain `jig update` intentionally moves to the current official branch. Use `jig update --recopy` to stay on the stored commit, or pass `--vcs-ref` to select a specific official ref.

**Develop against a local build:**

```sh
cargo build -p jig-sh --bin jig
JIG_DEV_BIN=target/debug/jig scripts/jig work status --summary
```

## Five-Minute Golden Path

Use this path when you want the fastest successful loop on a new or adopted repo.

1. Render the harness.

   ```sh
   jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
   # or, inside an existing repo:
   jig adopt . --repo-name existing-repo --sqlx-enabled false
   ```

2. Enter the repo and verify the pinned launcher works.

   ```sh
   cd /path/to/new-repo
   scripts/jig bootstrap
   scripts/jig check contract
   ```

3. Check local agent readiness. The JSON form is the stable automation output; `--summary` is the human scan path. `agent doctor` exits nonzero until required setup is complete.

   ```sh
   scripts/jig agent doctor --summary || true
   # If the summary reports missing marketplace registration:
   scripts/jig agent bootstrap
   scripts/jig agent doctor --summary
   ```

4. Start structured work, run required gates, and close it only after fresh evidence exists.

   ```sh
   plan_id="$(scripts/jig work start --title "First change" --body "Validate the harness loop." --print-plan-id)"

   scripts/jig work status --summary
   scripts/jig work check --plan-id "$plan_id"
   scripts/jig work gates --plan-id "$plan_id"
   scripts/jig work finish --plan-id "$plan_id" --resolution "Harness loop verified" --outcome success
   ```

5. For normal local validation, use the repo contract commands directly.

   ```sh
   scripts/jig check fmt
   scripts/jig check clippy
   scripts/jig check test
   ```

Contract and gate commands intentionally append receipts under `.agent/state/`.
Use `scripts/jig work status --summary` for a read-only scan of existing work
state and `scripts/jig work receipts --summary --failed-only` for a compact
receipt history. Pass `--no-receipt` to a one-off contract command when you do
not want evidence recorded.

## What It Generates

The template renders these repo-owned assets into a consumer repository:

- `.jig.toml`
- `.mcp.json`
- `AGENTS.md`
- `agent-map.md`
- `.agent/PLANS.md`
- `.agent/jig-contract.json`
- `Makefile` when `makefile_enabled = true`
- `scripts/*.sh`
- `scripts/enforce-coverage.js`
- `.github/workflows/*.yml`

Generated repos use `scripts/jig` as the execution backend. The generated `Makefile`, when enabled, is only a convenience adapter over `scripts/jig` and project-owned helper scripts.

On fresh machines, generated repos can check and bootstrap expected agent skills through the launcher:

```sh
scripts/jig agent doctor --summary
scripts/jig agent bootstrap
```

For local dogfooding with an existing sibling `jig-skills` checkout, pass `--marketplace ../jig-skills` to `agent bootstrap`.

The template does not generate application code, crate-level `AGENTS.md` files, or a schema dump implementation — those remain project-owned. SQLx and migration contract pieces are optional via `sqlx_enabled`; schema-check pieces are rendered only when schema dumps are enabled.

For existing repositories, root `AGENTS.md` remains repo-owned. `jig adopt` inserts or updates only the marked Jig-managed block and preserves the rest of the file.

## Templates

In Jig, `--template` means the source repository containing the harness files to render into another project. Release builds of `jig init` and `jig adopt` use the official `jig-sh` template at `https://github.com/bpcakes/jig-sh.git`, pinned to the release tag for the installed Jig version. Passing exactly `https://github.com/bpcakes/jig-sh` or `https://github.com/bpcakes/jig-sh.git` explicitly has the same pinned behavior unless `--vcs-ref` is also provided; SSH, fork, and private URLs follow normal remote-template behavior.

Pass `--template` only when you want to dogfood a local checkout, fork, or private template:

```sh
--template /path/to/jig-sh --template-mode committed
```

Unreleased or dirty local builds installed from a checkout refuse the implicit official `vVERSION` pin because that tag can describe older template code than the binary you are running. Dirty means tracked working-tree changes. Pass `--template /path/to/jig-sh --template-mode committed` to render from your checkout, or pass `--vcs-ref main` or another explicit official ref when you intentionally want remote template code.

Release automation that builds Jig from a git checkout must either fetch tags before building or set the build-time environment variable `JIG_ASSUME_RELEASE_BUILD=1` while running `cargo build` / `cargo install`, after validating the version and tag.

## Local Dev Proxy

Generated repos can run supervised development commands behind stable local hostnames.

### Running apps

```sh
scripts/jig dev
scripts/jig proxy alias api --port 8080
scripts/jig proxy list
```

`scripts/jig dev` runs configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or discovered workspace apps. It does not run the generic `dev_command`; keep repo-wide non-proxy dev orchestration in project-owned scripts or Make targets. Prefer `argv` for `[[dev.apps]]`; shell-form `command` runs through the platform shell from committed repo configuration and should be treated as trusted code execution. Apps with `proxy = false` run directly and do not publish Jig proxy routes.

### HTTPS setup

Generate and explicitly trust the local CA:

```sh
scripts/jig proxy cert generate
scripts/jig proxy cert trust --accept-trust-scope
```

To remove trust before regenerating or discarding a CA:

```sh
scripts/jig proxy cert untrust --accept-trust-scope
```

The `--accept-trust-scope` flag is required to acknowledge platform trust-store mutation. The CA is local and name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses. Keep `ca-key.pem` private and exclude the proxy state directory from backup or sync tools.

Automatic certificate generation, trust, and untrust are supported on macOS and Linux. On macOS, untrust removes matching Jig CA certificates from the login keychain by fingerprint. On Linux, Jig invokes the p11-kit and CA-refresh helpers from fixed system tool directories. Windows HTTPS certificate files are not written until owner-only ACL hardening is implemented.

### Platform notes

Process-owned proxy routes are supported on Linux and macOS. LAN mode binds the proxy on `0.0.0.0`; reachable LAN clients can use process-owned routes to supervised loopback apps, while aliases remain loopback-client-only.

On Windows and BSD-like platforms, run app commands directly with `scripts/jig proxy run --no-proxy`, or use `scripts/jig proxy alias` for manually managed loopback services. Automatic process-owned route publication is not available on these platforms.

The `jig-sh` crate enables the proxy stack by default. Library or MCP/contract-only consumers that do not need TLS/HTTP proxy dependencies can build with `default-features = false`; the `dev` and `proxy` CLI surfaces remain parseable but return clear unsupported-feature errors.

## Required Repo Conventions

All generated repos are expected to use:

- Cargo workspaces
- `cargo fmt`
- `cargo clippy`

When `sqlx_enabled` is `true`, repos are also expected to use:

- SQLx workspace metadata in a shared directory such as `.sqlx/`
- forward-only migration additions

Optional web apps are expected to expose these package scripts in each configured app directory:

- `lint`
- `typecheck`
- `build:bundle`
- `test:coverage`

The default workflow assumes Bun for package installation and script execution.

## Layout

- `crates/jig/` — publishable `jig` runtime and MCP server
- `crates/jig-dev-proxy/` — local HTTP/HTTPS proxy with TLS certificate management
- `templates/project/` — files rendered into downstream repos
- `docs/` — [configuration reference](docs/configuration.md), [adoption guide](docs/adoption.md), and [public contract documentation](docs/public-contract.md)
- `examples/` — visible `.jig.toml` answer-file examples and a short index
- `scripts/validate-fixtures.sh` — renders sample repos and validates the generated harness

## Validate This Repo

```sh
./scripts/validate-fixtures.sh
```

## Release

Use the GitHub Actions `Release` workflow — leave `version` blank for the next patch, or set it explicitly. See [CONTRIBUTING.md](CONTRIBUTING.md) for local release steps, CHANGELOG conventions, and crates.io trusted publishing setup.

## Contributing

Contributions welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
