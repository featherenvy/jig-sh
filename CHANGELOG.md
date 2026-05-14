# Changelog

## Unreleased

### Added
- Add Jig local development proxy commands for stable repo-scoped dev hostnames, HTTP/HTTPS forwarding, WebSocket support, workspace app discovery, local certificates, and service file generation.
- Add `scripts/jig dev` and `scripts/jig proxy {start,stop,list,prune,run,alias}` runtime flows for supervised app processes, aliases, and route listing/pruning.
- Add `scripts/jig proxy cert {generate,status,trust,untrust}` and `scripts/jig proxy service {install,status,uninstall}` for certificate trust management and user service installation; trust-store mutations require `--accept-trust-scope`, and `proxy service install` requires `--accept-service-scope`.
- Enable the `dev-proxy` Cargo feature by default while preserving `--no-default-features` builds for contract/MCP-only consumers.

### Changed
- BREAKING for local dogfooding: resolve `JIG_DEV_BIN` directly instead of copying it into the Jig cache, so local runtime changes use the current development binary after version validation.
- Hard-fail `scripts/install-jig.sh` when `JIG_DEV_BIN` is set but missing, non-executable, or resolves to a binary whose version does not match the generated repo instead of falling back to cached runtime selection. Direct callers of `scripts/install-jig.sh` should use `scripts/jig`, set a matching `JIG_DEV_BIN`, unset it, or run the normal cached installer path.
- Split the local development proxy runtime into the `jig-dev-proxy` workspace crate used by the `jig-sh` CLI.
- Refuse to share an unrelated proxy found on the requested HTTP port unless it is registered in the same proxy state directory.
- Prune legacy live process routes that do not have process start tokens on platforms where Jig can verify process start identity.
- BREAKING: Strictly reject unknown `.jig.toml` config fields so typos and stale local config fail fast.
- Migration note: remove or rename unknown `.jig.toml` keys reported by the load error before rerunning `scripts/jig`; previously ignored local keys now block startup. This applies to top-level keys plus `[work]`, `[agent_tooling]`, `[agent_tooling.codex]`, `[dev]`, `[[dev.apps]]`, and legacy `[[frontend_apps]]` entries.
- `jig-sh` now enables the `dev-proxy` feature by default, which pulls in the TLS/HTTP proxy stack for library consumers unless they opt into `default-features = false`.
- MCP/contract-only consumers can build with `default-features = false`; in that profile, `dev` and `proxy` still parse but return clear unsupported-feature errors instead of linking the proxy stack.
- Keep `web_package_manager = "bun"` as the default for legacy `[[frontend_apps]]`; configure `dev.apps` or set explicit commands when legacy apps should launch with another package manager.
- Require `--accept-trust-scope` before installing the Jig Dev Proxy local CA through the platform trust tooling.
- Vite proxy host support relies on Vite's `__VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS` compatibility hook; configure Vite `server.allowedHosts` explicitly if a Vite release changes that hook.
- Windows builds parse and run non-certificate proxy flows, but automatic HTTPS certificate generation/trust remains unsupported until owner-only ACL hardening for private key files is implemented.
- Document `JIG_PROXY_STATE_DIR`, proxy CA trust scope, and local dev proxy usage more explicitly.

### Security
- Harden proxy stop, certificate writes, CA regeneration, and TLS handshake behavior for local development sessions.
- Harden Vite argument handling, including rejection of mismatched explicit Vite port flags, backend response parsing, WebSocket proxy-header scrubbing, and route-cache invalidation.
- Harden LAN proxy exposure, alias registration, workspace discovery traversal, process-route liveness checks, and route persistence.
- Harden state directory permissions, service-file quoting, and local proxy shutdown behavior.
- Harden background proxy startup, runtime file replacement, request-host validation, installer locking, private-key reads, and workspace config file reads.
- Reverify process-route listener ownership while holding the route lock, restore the previous route file after failed route publication, isolate service temp paths, harden certificate/trust temporary reads, prefer recorded template commits for remote runtime installs, and defer release tag pushes until all crates publish successfully.
- Treat template source metadata as a runtime-install trust boundary: recorded hex `_commit` values pin the exact remote Jig revision used by `scripts/install-jig.sh`, and contract checks now keep the installer script and template mirror in sync.
- Bound the Jig Dev Proxy local CA lifetime to two years, avoid broad bare-TLD CA constraints for non-`.localhost` TLDs, and verify macOS trust installation before recording Jig's trusted-CA marker.
- Reject backend response headers with whitespace before the colon, retry transient TLS leaf cert/key rotation mismatches, escape `$` in systemd `ExecStart` values, fail closed on oversized workspace glob expansion, and extend proxy state lock waits.
- Document that shell-form `[[dev.apps]].command` is trusted repo-configured shell execution; prefer `argv` when arguments should be passed literally.

## v0.1.0 - 2026-05-12

### Added
- Scaffold agentic-rust-kit
- Add jig CLI tool and migrate to jig.sh branding
- Add template mode support for local git templates
- Add agent planning and workflow infrastructure
- Add state-summary tool and enhance receipts filtering
- Add block-managed root AGENTS.md to preserve repo-specific content during adoption

### Fixed
- Address extraction review findings
- Persist full copier template ref
- Unify kit config and update flow
- Make template source normalization safe

### Changed
- Split bootstrap module into separate concerns
- Split state module into separate concerns
- Extract tool definitions and remove memory tools from contract
- Extract bootstrap module concerns into separate files
- Improve encapsulation in template_source module
- Extract request parsing into runtime/requests module
- Extract work dispatch logic and improve runtime architecture
- Improve type safety, reduce bootstrap test cost, and organize runtime modules
- Move MCP tests to tests/mcp module and add Cargo.toml metadata

### Documentation
- Make copier update example noninteractive
- Distinguish recopy from update
- Make recopy noninteractive

### Tests
- Make fixture update check clean
- Fix update assertion
- Drop pyyaml dependency
- Add fixture infrastructure and agent documentation
- Refactor receipt creation and add plan state validation tests

### Other
- Improve work gate validation and receipt tracking
- Settle Cargo workspace in fixtures and document gate evidence requirements
- Add release script and normalize jig-sh package name
