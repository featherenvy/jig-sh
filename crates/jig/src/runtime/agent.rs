use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, bail};
use serde_json::{Value as JsonValue, json};

use crate::cli::{AgentBootstrapOpts, AgentCommand};
use crate::context::{CodexMarketplaceConfig, RepoContext};
use crate::process::{format_exit_status, require_success};

const CODEX_BIN_ENV: &str = "JIG_CODEX_BIN";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const JIG_SKILLS_MARKETPLACE_ENV: &str = "JIG_SKILLS_MARKETPLACE";

pub(super) fn dispatch(ctx: &RepoContext, command: AgentCommand) -> Result<JsonValue> {
    // Agent tooling commands describe or mutate local client setup, not repo
    // work evidence, so they intentionally do not record receipts.
    match command {
        AgentCommand::Doctor => doctor(ctx),
        AgentCommand::Bootstrap(opts) => bootstrap(ctx, opts),
    }
}

pub(super) fn doctor(ctx: &RepoContext) -> Result<JsonValue> {
    let codex_bin = codex_bin();
    let configured_marketplaces = ctx.codex_marketplaces();
    // Empty marketplace config intentionally means this repo has no Codex skill requirement.
    let codex_required = !configured_marketplaces.is_empty();
    let codex_available = codex_required.then(|| codex_supports_plugin_marketplaces(&codex_bin));
    let codex_ready = if codex_required {
        codex_available.unwrap_or(false)
    } else {
        true
    };
    let config_path = codex_config_path();
    let config = if codex_required {
        config_path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    } else {
        None
    };

    let marketplaces: Vec<JsonValue> = configured_marketplaces
        .iter()
        .map(|marketplace| marketplace_status(marketplace, config.as_ref(), ctx.root()))
        .collect();
    // `Iterator::all` is intentionally vacuously true here; `codex_required`
    // carries the empty-config opt-out semantics.
    let all_marketplaces_ready = marketplaces.iter().all(|marketplace| {
        marketplace
            .get("registered")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    });

    Ok(json!({
        "ok": codex_ready && all_marketplaces_ready,
        "command": "agent doctor",
        "codex": {
            "bin": codex_bin,
            "required": codex_required,
            "available": codex_available,
            "probe_skipped": !codex_required,
            "config_path": config_path.map(|path| path.display().to_string()),
            "config_read": config.is_some()
        },
        "readiness": {
            "ok_requires_marketplaces_registered": true,
            "ok_requires_plugins_enabled": false
        },
        "marketplaces": marketplaces
    }))
}

fn bootstrap(ctx: &RepoContext, opts: AgentBootstrapOpts) -> Result<JsonValue> {
    let codex_bin = codex_bin();
    let marketplace_source = requested_marketplace_source(ctx, opts.marketplace)?;
    let output = Command::new(&codex_bin)
        .args(["plugin", "marketplace", "add", &marketplace_source])
        .output()
        .with_context(|| {
            format!(
                "Failed to run {} plugin marketplace add {}",
                codex_bin, marketplace_source
            )
        })?;
    require_success(&output, |output| {
        codex_marketplace_add_failed_message(&codex_bin, &marketplace_source, output)
    })?;

    Ok(json!({
        "ok": true,
        "command": "agent bootstrap",
        "codex_bin": codex_bin,
        "marketplace_source": marketplace_source,
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr)
    }))
}

fn requested_marketplace_source(ctx: &RepoContext, explicit: Option<String>) -> Result<String> {
    if let Some(source) = explicit.or_else(|| env::var(JIG_SKILLS_MARKETPLACE_ENV).ok()) {
        return marketplace_source_for_codex(&source, ctx.root());
    }

    match ctx.codex_marketplaces() {
        [] => bail!(
            "No Codex marketplaces are configured in agent_tooling.codex.marketplaces; pass --marketplace <source> to install one explicitly"
        ),
        [marketplace] => marketplace_source_for_codex(&marketplace.source, ctx.root()),
        marketplaces => bail!(
            "Multiple Codex marketplaces are configured ({}); pass --marketplace <source> to choose one explicitly",
            marketplaces
                .iter()
                .map(|marketplace| format!("{}={}", marketplace.id, marketplace.source))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn marketplace_source_for_codex(source: &str, repo_root: &Path) -> Result<String> {
    let trimmed = source.trim();
    let path = Path::new(trimmed);
    let repo_relative_path = repo_root.join(path);
    if path.is_absolute() || trimmed.starts_with('.') {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            repo_relative_path
        };
        return resolved
            .canonicalize()
            .with_context(|| {
                format!(
                    "Configured Codex marketplace path {} does not exist from repo root {}",
                    source,
                    repo_root.display()
                )
            })
            .map(|path| path.display().to_string());
    }

    Ok(trimmed.to_string())
}

fn codex_marketplace_add_failed_message(
    codex_bin: &str,
    marketplace_source: &str,
    output: &Output,
) -> String {
    format!(
        "{} plugin marketplace add {} failed with {}\nstdout:\n{}\nstderr:\n{}",
        codex_bin,
        marketplace_source,
        format_exit_status(&output.status),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn marketplace_status(
    marketplace: &CodexMarketplaceConfig,
    config: Option<&toml::Value>,
    repo_root: &Path,
) -> JsonValue {
    // Current Codex plugin marketplace config is stored as
    // [marketplaces.<id>].source, with optional plugin diagnostics under
    // [plugins."<plugin id>"].enabled.
    let configured_marketplace = config
        .and_then(|config| config.get("marketplaces"))
        .and_then(|marketplaces| marketplaces.get(&marketplace.id));
    let configured_source = configured_marketplace
        .and_then(|marketplace| marketplace.get("source"))
        .and_then(toml::Value::as_str);
    let configured_source_type = configured_marketplace
        .and_then(|marketplace| marketplace.get("source_type"))
        .and_then(toml::Value::as_str);
    let source_matches = configured_source.is_some_and(|configured_source| {
        marketplace_source_matches(&marketplace.source, configured_source, repo_root)
    });
    let registered = configured_source.is_some() && source_matches;
    let plugins: Vec<JsonValue> = marketplace
        .plugins
        .iter()
        .map(|plugin| {
            let enabled = config
                .and_then(|config| config.get("plugins"))
                .and_then(|plugins| plugins.get(plugin))
                .and_then(|plugin| plugin.get("enabled"))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            json!({
                "id": plugin,
                "enabled": enabled
            })
        })
        .collect();
    let plugins_ready = plugins.iter().all(|plugin| {
        plugin
            .get("enabled")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    });

    json!({
        "id": marketplace.id,
        "source": marketplace.source,
        "configured_source": configured_source,
        "configured_source_type": configured_source_type,
        "registered": registered,
        "source_matches": source_matches,
        "plugins_ready": plugins_ready,
        "plugins": plugins
    })
}

fn marketplace_source_matches(expected: &str, configured: &str, repo_root: &Path) -> bool {
    normalized_marketplace_source(expected, repo_root)
        == normalized_marketplace_source(configured, repo_root)
}

fn normalized_marketplace_source(source: &str, repo_root: &Path) -> String {
    let trimmed = source.trim().trim_end_matches('/');
    let path = Path::new(trimmed);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    };
    if path.is_absolute() || trimmed.starts_with('.') {
        return resolved
            .canonicalize()
            .unwrap_or(resolved)
            .display()
            .to_string();
    }

    if let Some(github_source) = normalized_github_marketplace(trimmed) {
        return github_source;
    }

    // Keep diagnostics non-fatal: if a local path is currently missing, compare
    // against the repo-root-resolved display path and report source_matches.
    resolved
        .canonicalize()
        .unwrap_or(resolved)
        .display()
        .to_string()
}

fn normalized_github_marketplace(source: &str) -> Option<String> {
    let source = source
        .strip_prefix("https://github.com/")
        .or_else(|| source.strip_prefix("http://github.com/"))
        .or_else(|| source.strip_prefix("git@github.com:"))
        .unwrap_or(source)
        .trim_end_matches(".git")
        .trim_end_matches('/');
    let mut parts = source.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("github:{owner}/{repo}"))
}

fn codex_supports_plugin_marketplaces(codex_bin: &str) -> bool {
    // Codex does not expose a machine-readable feature probe for plugin
    // marketplaces, so doctor checks the concrete subcommand it later needs.
    Command::new(codex_bin)
        .args(["plugin", "marketplace", "add", "--help"])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn codex_bin() -> String {
    env::var(CODEX_BIN_ENV).unwrap_or_else(|_| "codex".into())
}

fn codex_config_path() -> Option<PathBuf> {
    let codex_home = env::var_os(CODEX_HOME_ENV)
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))?;
    Some(codex_home.join("config.toml"))
}
