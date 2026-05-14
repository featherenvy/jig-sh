# `.jig.toml` Configuration

This file is the supported configuration surface for downstream repos and must be committed alongside the generated template output.

`.jig.toml` is also the native renderer answers file.

After changing values in `.jig.toml`, re-render with:

```sh
jig update --recopy
```

To move onto a newer version of the template while keeping the stored answers, run:

```sh
jig update
```

For remote template sources, plain `jig update` advances to the remote default branch unless `--vcs-ref` is provided. `jig update --recopy` re-renders from the stored `_commit`.

The file contains both public settings and the private `_src_path` / `_commit` fields that `jig update` uses to resolve future renders. Repos rendered from local committed template checkouts may also store `_template_mode` and `_template_local_path`.

`jig update` refuses to overwrite or remove changed template-managed files unless `--force` is passed.

Root `AGENTS.md` is block-managed instead of file-managed. If the file already exists, `jig adopt` and `jig update` preserve user-authored content and insert or replace only the section between `<!-- BEGIN JIG MANAGED BLOCK -->` and `<!-- END JIG MANAGED BLOCK -->`. Edits inside that managed block are template-owned and may be replaced without `--force`; keep repo-specific guidance outside the markers.

`jig init` and `jig adopt` default to the official `jig-sh` template source at `https://github.com/bpcakes/jig-sh.git`, pinned to the release tag for the installed Jig version. Pass `--template` only when using a local checkout, fork, or private template.

For local git template checkouts, `jig init` / `jig adopt` use a committed source:

- `--template-mode committed`: explicitly use the clean local `HEAD`
- omit `--template-mode`: use the same committed local-template behavior

## Required Keys

- `repo_name`: display name used in generated docs
- `default_branch`: branch name used for base-ref comparisons
- `ci_github_runner`: runner label for GitHub Actions jobs
- `jig_version`: exact runtime version expected by generated repos
- `work.gates`: required work evidence gates evaluated before `scripts/jig work finish`
- `agent_tooling`: agent-client tooling expected for this repository, including Jig Codex skills
- `template_source_url`: optional canonical template source URL for portable recopy/update
- `sqlx_enabled`: whether to generate SQLx and migration-specific contract pieces
- `rust_crate_roots`: list of directories whose direct child directories are considered crates

When `sqlx_enabled` is `true`, these additional keys are required:

- `rust_migration_dir`: SQL migration directory
- `rust_sqlx_metadata_dir`: committed SQLx metadata directory

## Optional Keys

- `schema_dump_enabled`: when `true` and `sqlx_enabled` is also `true`, `make schema-check` executes `schema_dump_command`
- `schema_dump_command`: command that regenerates schema docs for SQLx-enabled repos
- `migration_add_command`: command behind `make migration-add` when `sqlx_enabled` is `true`
- `bootstrap_command`: implementation behind `make bootstrap`
- `dev_command`: implementation behind `make dev`
- `rust_fmt_check_command`
- `rust_clippy_command`
- `rust_test_command`
- `rust_test_locked_command`
- `web_package_manager`: currently `bun`
- `frontend_apps`: list of app definitions
- `dev`: Jig-native local development proxy settings and app definitions

## Accepted Key Summary

Jig rejects unknown `.jig.toml` keys so stale template answers fail early. The accepted top-level keys are `_src_path`, `_commit`, `_template_mode`, `_template_local_path`, `repo_name`, `default_branch`, `ci_github_runner`, `jig_version`, `template_source_url`, `sqlx_enabled`, `rust_crate_roots`, `rust_migration_dir`, `rust_sqlx_metadata_dir`, `schema_dump_enabled`, `schema_dump_command`, `migration_add_command`, `bootstrap_command`, `dev_command`, `rust_fmt_check_command`, `rust_clippy_command`, `rust_test_command`, `rust_test_locked_command`, `web_package_manager`, `frontend_apps`, `dev`, `work`, and `agent_tooling`.

Nested accepted keys are:

- `[[frontend_apps]]`: `name`, `dir`, `coverage_threshold`
- `[dev]`: `proxy_port`, `https_port`, `https`, `http2`, `lan`, `tld`, `workspace_discovery`, `apps`
- `[[dev.apps]]`: `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, `proxy`
- `[work]`: `checks`, `gates`, `refinements`
- `[[work.gates]]`: `id`, `kind`, `tool`, `skill`, `required`
- `[[work.refinements]]`: `id`, `skill`, `mode`
- `[agent_tooling.codex]`: `marketplaces`
- `[[agent_tooling.codex.marketplaces]]`: `id`, `source`, `plugins`

## `agent_tooling` Shape

The default rendered config declares the Jig Codex skills marketplace:

```toml
[[agent_tooling.codex.marketplaces]]
id = "jig-skills"
source = "bpcakes/jig-skills"
plugins = [
  "jig-rust@jig-skills",
  "jig-swift@jig-skills",
  "jig-typescript@jig-skills",
  "jig-exec-plans@jig-skills",
]
```

Jig Codex skills are optional Codex plugin bundles used by agents working in generated Jig repos; the default marketplace source is `bpcakes/jig-skills`.

Use `scripts/jig agent doctor` to report whether the local Codex installation can use the configured marketplace and to show diagnostic plugin enablement flags. The top-level `ok` result requires Codex marketplace support and registered marketplace sources; plugin enablement is reported separately because the supported Codex bootstrap path is marketplace registration. Use `scripts/jig agent bootstrap` to run `codex plugin marketplace add` when exactly one marketplace is configured. If multiple marketplaces are configured, `agent bootstrap` requires `--marketplace <source>` so a repo cannot install several user-level Codex marketplaces by default. `agent bootstrap` mutates user-level Codex config, so it is intentionally separate from the project-owned `bootstrap_command`.

Omitting `agent_tooling`, `agent_tooling.codex`, or `agent_tooling.codex.marketplaces` uses the default Jig skills marketplace. Set `marketplaces = []` to opt out explicitly. In `agent doctor` output, `codex.available` is `true` or `false` when Codex is required, and `null` when the Codex probe is skipped because `marketplaces = []`.

For local development against a sibling checkout, either pass `--marketplace` or set `JIG_SKILLS_MARKETPLACE`. Explicit `--marketplace` wins over `JIG_SKILLS_MARKETPLACE`, and the env var wins over `.jig.toml`; both overrides affect only `agent bootstrap`. Local path sources must be absolute or start with `./` or `../`; they are resolved from the repo root before Codex is invoked, and missing local paths fail before mutating Codex config. Bare `owner/repo` values are treated as marketplace shorthands, not local paths.

```sh
scripts/jig agent bootstrap --marketplace ../jig-skills
JIG_SKILLS_MARKETPLACE=../jig-skills scripts/jig agent bootstrap
```

## `work` Shape

The `work` block declares agent workflow defaults without adding repo-local launcher scripts:

```toml
[[work.gates]]
id = "contract"
kind = "check"
tool = "jig.contract_check"

[[work.gates]]
id = "tests"
kind = "check"
tool = "jig.test"
```

`kind: check` gates must reference make-backed jig tool names declared in `.agent/jig-contract.json`. `scripts/jig work check --plan-id ...` runs configured check gates in order unless one or more `--tool` values are passed explicitly.

`scripts/jig work gates --plan-id ...` reports each configured gate as `passed`, `missing`, `failed`, `stale`, `unknown`, or `unsupported`. `scripts/jig work finish --plan-id ...` refuses to close work while required gates are missing, failed, stale, unknown, or unsupported. Check gate freshness is based on the non-`.agent/` worktree fingerprint from the latest check or check-batch receipt that proves the gate.

Required check gates should not create or modify non-`.agent/` files during `work check`. Build outputs, generated metadata, and lockfiles should be committed when they are source-of-truth, ignored when they are disposable, or generated before running the fingerprinted check. If a check does intentionally settle generated files, rerun `scripts/jig work check --plan-id ...` after reviewing those changes so the gate evidence matches the final worktree.

After upgrading an in-flight repo from a Jig version that recorded receipts without `worktree_fingerprint`, rerun `scripts/jig work check --plan-id ...` before `scripts/jig work finish --plan-id ...`. Older successful check receipts deserialize correctly, but their freshness is `unknown` and required gates will block finish until fresh evidence exists.

For compatibility, older repos may still use `work.checks`; Jig backfills entries that are not already declared in `work.gates` as required `kind: check` gates with generated IDs. When a tool is declared in both places, the explicit `work.gates` entry is authoritative. New repos should use `work.gates`.

Generated SQLx-enabled repos also include check gates for `jig.sqlx_check` and `jig.schema_check`. Repos with schema dumps enabled also include `jig.schema_dump`.

Review procedures are intentionally separate from native check gates:

```toml
[[work.gates]]
id = "rust-error-handling"
kind = "codex_review"
skill = "jig-rust:rust-error-handling-review"
required = false
```

Codex-backed review gates are not implemented yet. They require a structured `codex exec --output-schema ...` receipt path before they can be required. Until then, non-`check` gates are reported as `unsupported` and block finish only when marked `required: true`.

`work.refinements` is reserved for future refinement execution. Current Jig versions reject it with a clear configuration error instead of accepting no-op refinement entries.

## `frontend_apps` Shape

Each entry in `frontend_apps` must be an object:

```toml
[[frontend_apps]]
name = "frontend"
dir = "frontend"
coverage_threshold = 40

[[frontend_apps]]
name = "admin-panel"
dir = "admin-panel"
coverage_threshold = 0
```

Each configured app directory is expected to support:

- install: `bun install --frozen-lockfile`
- lint: `bun run lint`
- typecheck: `bun run typecheck`
- build: `bun run build:bundle`
- test coverage: `bun run test:coverage`

## `dev` Shape

The `dev` table configures `scripts/jig dev` and `scripts/jig proxy`. This is runtime-owned local machine behavior, not a make-backed contract tool. Generated repos include a `[dev]` table with conservative defaults; repos that remove it use the runtime defaults from the pinned `jig_version`.

```toml
[dev]
proxy_port = 1355
https_port = 1443
https = false
http2 = true # HTTPS listener ALPN only; cleartext proxy traffic remains HTTP/1.1
lan = false
tld = "localhost"
workspace_discovery = false

[[dev.apps]]
name = "api"
kind = "env-port"
command = "cargo run --bin api"
port = 4000
proxy = true

[[dev.apps]]
name = "web"
dir = "apps/web"
kind = "vite"
argv = ["bun", "run", "dev"]
host = "127.0.0.1"
```

`proxy_port` is the TOML name for the HTTP listener. The matching CLI override is `--http-port`. `https_port` is the HTTPS listener; the matching CLI override is `--https-port`.

The `[dev]` table accepts these keys. Unknown keys are rejected so typos do not silently change local runtime behavior.

| Key | Type | Default | Scope |
| --- | --- | --- | --- |
| `proxy_port` | integer TCP port | `1355` | HTTP proxy listener |
| `https_port` | integer TCP port or omitted | `1443` | HTTPS proxy listener when `https = true` |
| `https` | boolean | `false` | enable the HTTPS listener |
| `http2` | boolean | `true` | enable HTTP/2 ALPN on the HTTPS listener |
| `lan` | boolean | `false` | bind the proxy to `0.0.0.0` for trusted LAN testing |
| `tld` | string | `"localhost"` | private/local route suffix |
| `workspace_discovery` | boolean | `false` | discover JavaScript workspace apps |
| `apps` | array of `[[dev.apps]]` tables | `[]` | supervised app definitions |

CLI runtime flags override listener defaults for a single invocation: use `--https`/`--no-https`, `--lan`/`--no-lan`, and the diagnostic `--http2`/`--no-http2` pair when needed.

Each `[[dev.apps]]` table accepts `name`, `dir`, `kind`, `command`, `argv`, `port`, `host`, and `proxy`. Unknown app keys are rejected. `name` is required; `dir`, `command`, `argv`, `port`, and `host` are optional; `kind` defaults to `"env-port"`; `proxy` defaults to `true`.

`tld` must use a private/local suffix: `localhost`, `local`, `test`, `internal`, or one subdomain beneath one of those suffixes such as `demo.test`. Public or browser-owned TLDs such as `dev`, `com`, and `io` are rejected so the proxy cannot mint routes that collide with routable DNS names.

Supported dev app kind values are `env-port` and `vite`.

`kind = "env-port"` starts the command with `PORT=<free-port>` and `HOST=127.0.0.1`, overriding inherited values so Jig controls apps that bind from those conventional variables. Framework-specific variables such as `VITE_PORT` or `SERVER_PORT` are not rewritten; configure those apps to honor `PORT`/`HOST`, fail on a busy port, or use a structured app kind that injects framework flags. `kind = "vite"` injects Vite-style `--port`, `--host`, and `--strictPort` flags when they are not already present. Jig also applies the same Vite flags to argv forms that directly invoke Vite, such as `vite`, `npx vite`, `bunx vite`, or `pnpm exec vite`. If a Vite argv already includes `-p` or `--port`, that value must match the Jig-assigned app port. For package-manager commands such as `bun run dev`, `pnpm run dev`, `npm run dev`, or `yarn run dev`, Jig inserts the `--` separator before Vite flags. Vite apps must use `argv`; shell-form `command` is rejected for Vite because safe flag injection requires structured arguments. If both `argv` and `command` are present, `argv` is used.

The Vite integration also sets Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` environment hook for the generated dev hostnames. Treat that hook as a Vite compatibility boundary: if your Vite version changes or removes it, use explicit Vite `server.allowedHosts` config or `kind = "env-port"` until Jig is updated.

Do not configure both `[[dev.apps]]` and legacy `[[frontend_apps]]` in the same repo. Legacy `[[frontend_apps]]` entries are still supported as a fallback when no `dev.apps` are configured, so older generated repos can use the proxy without duplicate app names.

To migrate a legacy frontend entry, create a matching `[[dev.apps]]` entry with the same `name` and `dir`, set `kind = "vite"` for Vite-style frontends, and set `argv` to the package-manager dev command such as `["bun", "run", "dev"]`. `coverage_threshold` stays with the older frontend check workflow and is not used by `scripts/jig dev`; keep any build, lint, typecheck, or coverage commands in project-owned make targets.

Jig rejects unknown top-level `.jig.toml` keys and unknown keys inside known tables. During upgrades, remove experimental keys or move repo-local notes outside `.jig.toml`; template-owned compatibility keys are listed in the required and optional sections above.

When `workspace_discovery = true`, Jig discovers common JavaScript workspace package globs under the repo root after `JIG_DEV_ALLOW_WORKSPACE_DISCOVERY=1` is present in the environment, because discovered package `dev` scripts are executable repo code. The matching one-shot CLI override is `scripts/jig dev --discover-workspace`. Discovery supports `*`, `**`, and leading `!` exclusions, but not brace expansion such as `apps/{web,admin}`. Discovery skips `node_modules`, dot-directories, symlinked package directories, and canonical paths outside the repo root. Glob expansion fails closed after 10,000 matches; narrow workspace globs in very large monorepos.

`scripts/jig dev` only launches configured `[[dev.apps]]`, legacy `[[frontend_apps]]`, or workspace-discovered apps. It does not run the generic top-level `dev_command`; keep `make dev` for repo-wide commands that do not bind a supervised app port.

Each dev app defaults to `host = "127.0.0.1"` and `proxy = true` unless a `[[dev.apps]]` entry overrides them. This is the backend target host that Jig forwards to, not the proxy listener address. Proxied dev apps, including legacy `[[frontend_apps]]` entries, must target loopback IP literals such as `127.0.0.1` or `::1`; use `scripts/jig proxy alias` for deliberate proxied non-loopback local tunnels. A `proxy = false` app is launched directly without publishing any Jig proxy route, so its `host` value is only passed to the child process as the app bind target. Set `port` only when an app must use a fixed backend port; otherwise Jig assigns a free port in the local app range.

When running multiple apps with `scripts/jig dev`, Jig treats them as a tied dev session: when the first child process exits, Jig removes the session routes and terminates the remaining child processes.

Jig chooses automatic app ports with a local bind probe against every socket address resolved from each app's configured target host, then starts the child command and waits for the target port before publishing the process route. The probe does not reserve the socket across arbitrary package scripts, so Jig verifies that the observed listener belongs to the spawned child process group before publishing. If a concurrent local process steals the port or the app rebinds to a different port, Jig reports an app readiness failure instead of publishing the bad route.

`scripts/jig proxy run` uses `--` before the command to run, for example `scripts/jig proxy run web -- vite --open`. Use `scripts/jig proxy run web --no-proxy -- <command>` when you only want Jig to assign `PORT`/`HOST` and supervise the process without registering a proxy route. App execution options such as `--kind` and `--port` still affect that direct child process; proxy listener options such as `--http-port`, `--https-port`, `--https`, `--lan`, and `--tld` are rejected with `--no-proxy`.

`scripts/jig dev` and `scripts/jig proxy run` execute repository-configured commands and package scripts. Only run them in repositories you trust.
`[[dev.apps]]` commands and `proxy run -- <command>` are executed verbatim from the configured app directory with Jig-provided `PORT` and `HOST` values, and they inherit the invoking shell environment so ordinary dev credentials and toolchain variables keep working. The string `command` form runs through the platform shell from committed repo configuration; use it only for trusted repos and prefer `argv` for literal argument passing. The long-running background proxy is different: Jig launches it with a cleared environment plus explicit proxy state and minimal toolchain variables, and Unix background starts detach from the caller's working directory. Apps that prefer framework-specific port variables must be configured to derive them from `PORT` or use a Jig app kind that supplies equivalent flags.

The proxy process is shared local runtime state and can outlive a `scripts/jig dev` session. Use `scripts/jig proxy stop` when you want to shut down the background proxy listener. Proxy commands print their JSON response before returning; when a response contains `ok: false`, including stop refusals that deliberately keep runtime files to avoid terminating an unrelated process, the CLI exits nonzero so scripts should inspect the JSON warning field.

The proxy is intended for trusted local development and trusted LAN testing only; it does not provide authentication or multi-tenant isolation.

Routes and aliases are repo-scoped by default. For example, in a repository named `demo`, `scripts/jig proxy alias api --port 8080` registers `api.demo.localhost`, matching the same certificate wildcard used by `scripts/jig dev` apps.

Aliases default to `127.0.0.1`. If you pass `--host`, it must be an IP literal; DNS names are rejected so alias routing cannot depend on mutable hostname resolution. Non-loopback alias targets require `--accept-non-loopback-target`; treat those aliases as local access grants to that target IP and avoid pointing them at sensitive internal services. In LAN mode, aliases may only target loopback IP literals so `0.0.0.0` proxy binding cannot expose arbitrary internal hosts.

Forwarded HTTP and WebSocket requests use the routed development hostname in `Host` and `x-forwarded-host`. Jig replaces inbound `x-forwarded-for` with the direct client address instead of trusting client-supplied chains. Apps that enforce hostnames should allow the generated route names.

Proxy forwarding appends a standard `Via` hop marker to HTTP and WebSocket requests and responses, using the inbound protocol version such as `1.1 jig` or `2.0 jig`. HTTP request bodies are streamed with a 100 MiB forwarding limit, so large upload workflows should bypass the proxy or raise that limit in code. Backend HTTP requests are normalized to HTTP/1.1 even when the client reaches the TLS listener over HTTP/2; HTTP/1 keep-alive is disabled so each HTTP/1 client request uses a fresh connection. The health endpoint requires a loopback client address, a loopback `Host` value, and the per-run health token stored in the private proxy state directory. When the connection limit is reached, the proxy applies backpressure before accepting more sockets, so clients may wait in the OS backlog or time out; slow TLS handshakes are bounded by a short handshake timeout. HTTP/2 is additionally bounded by the configured max concurrent streams per connection and a global active-request limit.

Non-upgrade WebSocket backend responses are drained with a bounded 10 MiB body limit so error pages can be returned without allowing unbounded buffering.

The proxy stores mutable local state under `~/.jig/proxy` by default, or `JIG_PROXY_STATE_DIR` when set. Commands that accept proxy runtime flags also accept `--state-dir <path>` for explicit per-call isolation. This state is deliberately outside `.agent/state` because routes, PID files, port files, certificates, and advisory lock files are mutable machine-local runtime data. Route state is versioned JSON. Route hostnames are shared state-dir keys: if multiple repos use the same state directory and hostname, Jig treats that as the same route and refuses live process-route replacement. Shared state directories reuse one leaf certificate and add hostnames for live routes and aliases, so use separate state directories when many repos or aliases would otherwise make the certificate SAN list too large. State mutations use advisory locks and wait up to 30 seconds before reporting a lock timeout. On Unix, Jig makes the newly created default state parent (`~/.jig`) and the default, newly created, or empty explicit state directory mode `700`; existing default parents and existing non-empty explicit state directories must already be mode `700`. Windows ACL hardening is not implemented.

A proxy 404 lists routes in the active state directory to help local loopback debugging. Non-loopback clients receive a hidden route list. Use separate `JIG_PROXY_STATE_DIR` values when you do not want unrelated repos to share route listings.

Route and certificate caches include file-content signatures plus a short freshness window. Normal route and certificate writes are picked up immediately; a stale read should last no longer than about 500 ms. Dead process routes are filtered from live reads; run `scripts/jig proxy prune` when you want that cleanup persisted to the route file immediately.

`lan = true` binds the proxy to the IPv4 wildcard address `0.0.0.0` and reports the detected LAN IP address when one is available. The detected IPv4 LAN address is captured when the proxy starts and is also used by the proxy self-loop guard, so restart the proxy after changing networks before relying on the new address. Other devices still need a DNS or hosts-file entry for repo-scoped names such as `web.demo.localhost`, or they must send that hostname as the HTTP `Host` header.
LAN IP detection connects an unbound UDP socket to `8.8.8.8:80` to select the outbound interface without sending application data to that address.

On the local machine, the default `.localhost` names resolve to loopback automatically. If you configure `tld = "test"`, `tld = "local"`, or `tld = "internal"`, add hosts-file, DNS, or mDNS resolution for the generated repo-scoped names before expecting browsers to resolve them. Certificates are generated for Jig route hostnames and explicitly configured additional DNS names; custom multi-label TLDs do not imply that the bare TLD itself is covered. Wildcard additional DNS names such as `*.demo.localhost` add the stripped subtree (`demo.localhost`) to the local CA name constraints, so keep them repo-scoped rather than broad.

Generated leaf certificate PEM files contain the leaf certificate. Trust-aware clients should import the Jig local CA through `scripts/jig proxy cert trust --accept-trust-scope` or configure their trust store with the generated CA certificate.

LAN mode exposes the Jig proxy, not the child app process directly. Jig still starts child apps on loopback IP literals (`HOST=127.0.0.1` or Vite `--host 127.0.0.1`) and routes LAN traffic through the proxy. Health and administrative endpoints remain loopback-only. When HTTPS and LAN mode are both enabled, Jig includes the detected IPv4 LAN address in the generated certificate names when one is available; switching networks can change that IP, so regenerate the leaf certificate if browsers report a name mismatch after moving networks. For `tld = "local"` in LAN mode, Jig does not add broad `local` or `*.local` certificate names; use repo-scoped route hostnames or explicit additional DNS names instead.

In LAN mode, Jig-owned process routes remain reachable because Jig starts and supervises their child apps on loopback IP literals. Alias routes remain loopback-client-only; local loopback clients can still use those aliases for remote tunnels or shared development services.

On Windows and BSD-like platforms, child-process tree cleanup is best-effort, and process-owned proxy routes are refused because high-confidence process start-token verification is not implemented yet. `scripts/jig proxy stop` on Windows only terminates a process after the authenticated loopback health endpoint reports the stored PID, but it cannot recheck a process start token before `taskkill`; stop long-running Windows proxies promptly if you are relying on PID identity. HTTPS dev proxying is not supported on Windows because Jig does not yet harden private-key ACLs there; use plain HTTP, `scripts/jig proxy run --no-proxy`, or `scripts/jig proxy alias` for manually managed loopback services.

If a proxy is already running without HTTPS and a later command asks for HTTPS, stop and restart the proxy with HTTPS using the same `JIG_PROXY_STATE_DIR`. Use separate state directories for worktrees that need different HTTP/HTTPS listener settings.

Ports below `1024`, including `80` and `443`, usually require elevated bind privileges on Unix-like systems. Jig attempts the bind and reports the OS error with an actionable hint when it fails. On Linux, grant the installed Jig binary `cap_net_bind_service`; on macOS, use a root-owned LaunchDaemon or forward 80/443 to unprivileged Jig proxy ports.

Useful commands:

- `scripts/jig dev`
- `scripts/jig dev --app web`
- `scripts/jig proxy start`
- `scripts/jig proxy stop`
- `scripts/jig proxy list`
- `scripts/jig proxy prune`
- `scripts/jig proxy run web -- vite`
- `scripts/jig proxy alias api --port 8080`
- `scripts/jig proxy cert generate`
- `scripts/jig proxy cert status`
- `scripts/jig proxy cert trust --accept-trust-scope`
- `scripts/jig proxy cert untrust --accept-trust-scope`
- `scripts/jig proxy service install --accept-service-scope`
- `scripts/jig proxy service status`
- `scripts/jig proxy service uninstall`

`scripts/jig proxy service install --accept-service-scope` writes the user service file and attempts to load/start it with the platform service manager after you acknowledge that scope. Jig invokes the service manager from fixed system tool locations rather than the invoking shell's `PATH`. It refuses to overwrite an existing service file with different contents; uninstall or remove that file before installing a changed definition. `scripts/jig proxy service uninstall` attempts to stop/unload it before removing the file and keeps the file in place when unloading fails.

Service installation records the canonical path of the currently running `jig` executable. Verify the launcher path before installing or reinstalling the service, especially after replacing a local development binary.

`scripts/jig proxy cert trust --accept-trust-scope` installs a local CA through the platform trust tooling after acknowledging the trust scope. On macOS Jig targets the login keychain. On Linux Jig uses p11-kit `trust anchor` when available and then refreshes CA bundles with the distribution command it finds in fixed system tool directories; depending on distribution policy, those Linux steps may use a user trust store or require privileges. The CA is name-constrained to configured Jig development DNS names plus loopback and detected IPv4 LAN addresses, but `ca-key.pem` is still sensitive local TLS material. Keep it private, exclude the proxy state directory from backup or sync tools that may copy private keys outside local filesystem permissions, use a dedicated `JIG_PROXY_STATE_DIR` when isolation matters, and do not trust the CA unless HTTPS proxying needs browser trust.

`scripts/jig proxy cert untrust --accept-trust-scope` removes matching Jig Dev Proxy Local CA certificates by fingerprint where the platform tooling can manage them after you acknowledge platform trust-store mutation. On macOS this deletes matching certificates from the login keychain rather than only toggling trust settings; manually remove copies installed in other keychains, and run the command again if it reports that more matching certificates may remain. On Linux, Jig removes the current CA's exact p11-kit trust anchor when `trust list --filter=ca-anchors` reports it and refreshes the system CA bundle with `update-ca-trust extract` or `update-ca-certificates` when one is available; distribution policy determines whether those steps need privileges. Automatic certificate generation, trust, and untrust are not implemented on Windows until owner-only ACL hardening for private key files is available.

If the local CA key may be compromised, run `scripts/jig proxy cert untrust --accept-trust-scope` before `scripts/jig proxy cert generate --force`, then trust the regenerated CA only if needed. On macOS, `generate --force` refuses to replace a currently trusted Jig CA by fingerprint so an old trusted root is not orphaned. On Linux, Jig also checks p11-kit's trusted CA anchor list for a Jig Dev Proxy CA label when `trust` is available. On other platforms, Jig records successful Jig-managed trust operations in the state directory and refuses forced replacement while that marker still matches the current CA.

## Generated Contract

The compatibility policy for generated make-backed CLI commands, MCP tools, and `.agent/jig-contract.json` is defined in [Public Contract](./public-contract.md).

The generated `Makefile` exposes these stable targets:

- `bootstrap`
- `deps`
- `dev`
- `fmt-check`
- `clippy`
- `test-rust`
- `test-rust-locked`
- `test`
- `contract-check`
- `check-agent-map`
- `check-agent-guides`
- `check-rust-file-loc`
- `check-no-mod-rs`
- `ci`

When `sqlx_enabled` is `true`, generated repos also expose:

- `sqlx-db-setup`
- `sqlx-check`
- `schema-check`
- `migration-add`
- `check-sqlx-unchecked-non-test`

When both `sqlx_enabled` and `schema_dump_enabled` are `true`, generated repos also expose:

- `schema-dump`

Downstream repos may add more targets, but these names should remain stable for agent tooling.

Generated repos also get these runtime-owned files:

- `.mcp.json`
- `.agent/jig-contract.json`
- `scripts/jig`
- `scripts/install-jig.sh`

The generated `scripts/jig` launcher enforces the exact `jig_version` pinned in `.jig.toml`. On first use it installs that version into a repo-local cache and then exposes the make-backed contract as:

- CLI commands such as `scripts/jig fmt-check`
- MCP tools such as `jig.fmt_check`

It also provides runtime-owned append-only memory under `.agent/state/*.jsonl` through the structured work namespace:

- `scripts/jig agent doctor`
- `scripts/jig agent bootstrap`
- `scripts/jig work start --title ...`
- `scripts/jig work append --plan-id ...`
- `scripts/jig work check --plan-id ...`
- `scripts/jig work gates --plan-id ...`
- `scripts/jig work decide --plan-id ...`
- `scripts/jig work receipts --plan-id ...`
- `scripts/jig work status`
- `scripts/jig work finish --plan-id ...`

`work finish` closes the plan with `--resolution`. If an active session is also open, it closes that session with `--outcome`; when `--outcome` is omitted, the session outcome falls back to `--resolution`.

For local runtime development, set `JIG_DEV_BIN` to an already-built `jig` binary. The installer resolves that explicit binary to an absolute path before returning it, and verifies that its reported version matches `.jig.toml`. A stale or mismatched `JIG_DEV_BIN` is a hard error rather than a fallback to the cached runtime. Avoid rebuilding that binary while a long-running `JIG_DEV_BIN` process, such as `jig proxy start --foreground`, is still active.

## SQLx Metadata Directory

This section applies only when `sqlx_enabled` is `true`.

`rust_sqlx_metadata_dir` is wired into the generated `sqlx-check` target via `SQLX_OFFLINE_DIR`. Use `.sqlx` unless the repository has already standardized on a different committed metadata path.

## Template Source

For portable shared repos, set:

```toml
template_source_url = "git@github.com:your-org/jig-sh.git"
```

When `template_source_url` is set, the renderer writes it into `_src_path` for portable update and install behavior. If it is blank, local template renders keep the local source path.
