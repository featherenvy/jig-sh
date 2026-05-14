# jig.sh

[![Tests](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/bpcakes/jig-sh/actions/workflows/rust-tests.yml)
[![Crates.io](https://img.shields.io/crates/v/jig-sh)](https://crates.io/crates/jig-sh)

Jig turns a Rust application repo into an operating environment for coding agents. Without it, agents lose context across machines, lack a stable execution contract, and leave no inspectable record of their work. Jig fixes that by generating the scaffolding once and keeping it in sync.

It makes agentic software work repeatable, inspectable, and reviewable by generating:

- **Agent context files** (`AGENTS.md`, `agent-map.md`) so coding agents know the repo layout and conventions without reading source
- **A `Makefile` contract** with a fixed set of top-level targets so every machine, CI run, and agent executes the same commands
- **A typed `jig` runtime** wrapping that contract so agents call validated commands and leave an append-only receipt under `.agent/state/`
- **A local dev proxy** so app hostnames stay stable across port changes and machine restarts
- **Work gates backed by receipts** so a task cannot be marked done without a verifiable output artifact
- **Repo policy scripts and CI workflows** so linting, tests, and coverage enforcement run consistently from day one
- **Template sync via `jig update`** so the harness stays current as `jig-sh` evolves — without overwriting files you have customized

## How It Works

Jig's template lives in the `jig-sh` repository. Running `jig init` or `jig adopt` renders it into your project, producing a `Makefile`, agent context files, CI workflows, and MCP configuration. After that first render, `jig update` keeps those files current as the template evolves — files you have customized are never overwritten without `--force`.

## Quick Start

**Prerequisites:** Rust 1.85+, [Bun](https://bun.sh) (web targets), Postgres (when `sqlx_enabled = true`)

```sh
cargo install jig-sh
```

Generated repos install and pin their own `jig` version automatically via `scripts/install-jig.sh`. You only need a global install to run `jig init` or `jig adopt` on a repo for the first time.

By default, `jig init` and `jig adopt` clone the official template from GitHub. For offline use or local head dogfooding, pass `--template /path/to/jig-sh`.

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
JIG_DEV_BIN=target/debug/jig scripts/jig work status
```

## What It Generates

The template renders these repo-owned assets into a consumer repository:

- `.jig.toml`
- `.mcp.json`
- `AGENTS.md`
- `agent-map.md`
- `.agent/PLANS.md`
- `.agent/jig-contract.json`
- `Makefile`
- `scripts/*.sh`
- `scripts/enforce-coverage.js`
- `.github/workflows/*.yml`

Generated repos keep `make` as the execution backend, but they also get a `scripts/jig` launcher, MCP wiring, and append-only repo memory under `.agent/state/*.jsonl`.

On fresh machines, generated repos can check and bootstrap expected agent skills through the launcher:

```sh
scripts/jig agent doctor
scripts/jig agent bootstrap
```

For local dogfooding with an existing sibling `jig-skills` checkout, pass `--marketplace ../jig-skills` to `agent bootstrap`.

The template does not generate application code, crate-level `AGENTS.md` files, or a schema dump implementation — those remain project-owned. SQLx, migration, and schema-check contract pieces are optional via `sqlx_enabled`.

For existing repositories, root `AGENTS.md` remains repo-owned. `jig adopt` inserts or updates only the marked Jig-managed block and preserves the rest of the file.

## Templates

In Jig, `--template` means the source repository containing the harness files to render into another project. By default, `jig init` and `jig adopt` use the official `jig-sh` template at `https://github.com/bpcakes/jig-sh.git`, pinned to the release tag for the installed Jig version. Passing exactly `https://github.com/bpcakes/jig-sh` or `https://github.com/bpcakes/jig-sh.git` explicitly has the same pinned behavior unless `--vcs-ref` is also provided; SSH, fork, and private URLs follow normal remote-template behavior.

Pass `--template` only when you want to dogfood a local checkout, fork, or private template:

```sh
--template /path/to/jig-sh
```

Prerelease or development builds still try the exact `vVERSION` tag for that binary. If that tag has not been published yet, pass `--vcs-ref main` or use a local checkout with `--template /path/to/jig-sh`.

## Local Dev Proxy

Generated repos can run supervised development commands behind stable local hostnames.

### Running apps

```sh
scripts/jig dev
scripts/jig proxy alias api --port 8080
scripts/jig proxy list
```

`scripts/jig dev` runs configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or discovered workspace apps. It does not run the generic `dev_command`; keep `make dev` for repo-wide commands that do not bind a supervised app port. Prefer `argv` for `[[dev.apps]]`; shell-form `command` runs through the platform shell from committed repo configuration and should be treated as trusted code execution. Apps with `proxy = false` run directly and do not publish Jig proxy routes.

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
- `examples/` — example `.jig.toml` answer files
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
