use std::env;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(feature = "dev-proxy"))]
use std::process::Command;

#[cfg(not(feature = "dev-proxy"))]
use anyhow::anyhow;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

#[cfg(feature = "dev-proxy")]
use crate::command::{ProxyCommand, ProxyListRequest, ProxyRuntimeOptions};
use crate::command::{VaultCommand, VaultRuntimeOptions, VaultStatusRequest};
use crate::context::{RepoContext, find_repo_root_from};
use crate::tool_defs::tool;

const COMMAND: &str = "doctor";

pub(crate) fn run() -> Result<Value> {
    let cwd = env::current_dir().context("Failed to resolve current directory")?;
    let root_result = find_repo_root_from(&cwd);
    let mut checks = Vec::new();

    let root = match root_result {
        Ok(root) => root,
        Err(error) => {
            checks.push(check(
                "repo",
                "Jig repo",
                true,
                false,
                "missing",
                error.to_string(),
            ).with_fix("Run `scripts/jig adopt . --repo-name <name> --sqlx-enabled false` from the repository root, or run `scripts/jig init <path> --repo-name <name> --sqlx-enabled false` to create a new repo."));
            return Ok(output(None, checks));
        }
    };

    checks.push(
        check(
            "repo",
            "Jig repo",
            true,
            true,
            "found",
            root.display().to_string(),
        )
        .with_data(json!({ "root": root.display().to_string() })),
    );

    let config_probe = RepoContext::validate_config_file(&root);
    let ctx_result = RepoContext::load_from_root(root.clone());
    let (config_ok, repo_name, config_jig_version) = match &config_probe {
        Ok(probe) => (
            true,
            Some(probe.repo_name.clone()),
            Some(probe.jig_version.clone()),
        ),
        Err(_) => (false, None, None),
    };
    checks.push(config_check(&root, &config_probe));
    checks.push(runtime_check(&root, config_jig_version.as_deref()));

    match &ctx_result {
        Ok(ctx) => {
            checks.push(contract_check(ctx));
            checks.push(required_tools_check(ctx));
            checks.push(agent_check(ctx));
            checks.push(proxy_check(ctx));
        }
        Err(error) => {
            let context_error = if config_ok {
                format!("Repo context failed to load: {error}")
            } else {
                format!("Skipped until .jig.toml is valid: {error}")
            };
            checks.push(
                check(
                    "contract",
                    "Contract",
                    true,
                    false,
                    "blocked",
                    context_error.clone(),
                )
                .with_fix("Run `scripts/jig check contract --no-receipt` after fixing the reported repo configuration issue."),
            );
            checks.push(
                check(
                    "required_tools",
                    "Required tools",
                    true,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig check contract --no-receipt` first."),
            );
            checks.push(
                check(
                    "agent_skills",
                    "Agent skills",
                    true,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor --summary` after fixing the contract issue."),
            );
            checks.push(
                check(
                    "proxy",
                    "Proxy",
                    false,
                    false,
                    "blocked",
                    format!("Skipped until repo context loads successfully: {context_error}"),
                )
                .with_fix("Run `scripts/jig doctor --summary` after fixing the contract issue."),
            );
        }
    }

    checks.push(vault_check());

    Ok(output(
        Some(json!({
            "root": root.display().to_string(),
            "name": repo_name,
            "jig_version": config_jig_version,
        })),
        checks,
    ))
}

pub(crate) fn format_summary(value: &Value) -> String {
    let ready = value["ok"].as_bool().unwrap_or(false);
    let mut lines = vec![format!(
        "Jig doctor: {}",
        if ready { "ready" } else { "needs attention" }
    )];
    if let Some(root) = value["repo"]["root"].as_str() {
        lines.push(format!("Repo: {root}"));
    }
    lines.push("Checks:".into());
    for check in value["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let label = check["label"].as_str().unwrap_or("<unknown>");
        let status = check["status"].as_str().unwrap_or("unknown");
        let required = check["required"].as_bool().unwrap_or(false);
        let required_label = if required { "required" } else { "optional" };
        let marker = if check["ok"].as_bool().unwrap_or(false) {
            "ok"
        } else {
            "needs setup"
        };
        lines.push(format!(
            "  - {label}: {marker} ({status}, {required_label})"
        ));
    }

    match value["next_step"].as_str() {
        Some(step) => lines.push(format!("Next step: {step}")),
        None => lines.push("Next step: none".into()),
    }
    lines.join("\n")
}

fn output(repo: Option<Value>, checks: Vec<DoctorCheck>) -> Value {
    let required_ok = checks.iter().all(|check| !check.required || check.ok);
    let next_issue = checks
        .iter()
        .find(|check| check.required && !check.ok)
        .or_else(|| checks.iter().find(|check| !check.ok));
    let next_step = next_issue.and_then(|check| check.fix.clone());
    let next_issue = next_issue.map(|check| {
        json!({
            "id": &check.id,
            "label": &check.label,
            "required": check.required,
            "status": &check.status,
            "fix": &check.fix,
        })
    });
    let checks = serde_json::to_value(checks).expect("doctor checks serialize");

    json!({
        "ok": required_ok,
        "command": COMMAND,
        "repo": repo,
        "checks": checks,
        "next_issue": next_issue,
        "next_step": next_step,
    })
}

fn config_check(root: &Path, result: &Result<crate::context::RepoConfigProbe>) -> DoctorCheck {
    match result {
        Ok(probe) => check(
            "config",
            ".jig.toml",
            true,
            true,
            "valid",
            format!(
                "repo_name={}, jig_version={}",
                probe.repo_name, probe.jig_version
            ),
        )
        .with_data(json!({
            "path": root.join(".jig.toml").display().to_string(),
            "repo_name": probe.repo_name,
            "jig_version": probe.jig_version,
        })),
        Err(error) => check(
            "config",
            ".jig.toml",
            true,
            false,
            "invalid",
            error.to_string(),
        )
        .with_fix("Fix `.jig.toml`, then run `scripts/jig doctor --summary`.")
        .with_data(json!({ "path": root.join(".jig.toml").display().to_string() })),
    }
}

fn runtime_check(root: &Path, config_jig_version: Option<&str>) -> DoctorCheck {
    let current_version = env!("CARGO_PKG_VERSION");
    let script_path = root.join("scripts/jig");
    let launcher = launcher_version(&script_path);
    let script_version = launcher.version;
    let script_ok = script_path.exists();
    let launcher_ok = launcher.read_error.is_none()
        && script_version
            .as_deref()
            .is_none_or(|version| version == current_version);
    let config_ok = config_jig_version.is_none_or(|version| version == current_version);
    let version_ok = launcher_ok && config_ok;
    let ok = script_ok && version_ok;
    let detail = match (
        &script_version,
        launcher.read_error.as_deref(),
        config_jig_version,
    ) {
        (_, Some(error), Some(config_version)) => {
            format!(
                "running {current_version}, scripts/jig is unreadable ({error}), .jig.toml pins {config_version}"
            )
        }
        (_, Some(error), None) => {
            format!("running {current_version}, but scripts/jig is unreadable ({error})")
        }
        (Some(script_version), None, Some(config_version)) => {
            format!(
                "running {current_version}, launcher pins {script_version}, .jig.toml pins {config_version}"
            )
        }
        (Some(script_version), None, None) => {
            format!("running {current_version}, launcher pins {script_version}")
        }
        (None, None, Some(config_version)) if script_ok => format!(
            "running {current_version}, scripts/jig has no readable JIG_VERSION pin, .jig.toml pins {config_version}"
        ),
        (None, None, None) if script_ok => {
            format!("running {current_version}, but scripts/jig has no readable JIG_VERSION pin")
        }
        (None, None, _) => format!("running {current_version}, but scripts/jig is missing"),
    };
    let status = if ok && script_version.is_none() {
        "unverified launcher"
    } else if ok {
        "installed"
    } else {
        "mismatch"
    };
    let fix = if !script_ok || !version_ok {
        Some("Run `scripts/jig update`, then rerun `scripts/jig doctor --summary`.")
    } else {
        None
    };

    check("runtime", "Pinned runtime", true, ok, status, detail)
        .with_optional_fix(fix)
        .with_data(json!({
                "current_version": current_version,
                "launcher_path": script_path.display().to_string(),
                "launcher_version": script_version,
                "launcher_error": launcher.read_error,
                "config_jig_version": config_jig_version,
        }))
}

struct LauncherVersion {
    version: Option<String>,
    read_error: Option<String>,
}

fn launcher_version(path: &Path) -> LauncherVersion {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LauncherVersion {
                version: None,
                read_error: None,
            };
        }
        Err(error) => {
            return LauncherVersion {
                version: None,
                read_error: Some(error.to_string()),
            };
        }
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("JIG_VERSION=") else {
            continue;
        };
        return LauncherVersion {
            version: Some(unquote_shell_value(value.trim()).to_string()),
            read_error: None,
        };
    }
    LauncherVersion {
        version: None,
        read_error: None,
    }
}

fn unquote_shell_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn contract_check(ctx: &RepoContext) -> DoctorCheck {
    match crate::policy::contract_check(ctx) {
        Ok(output) if output.exit_status == 0 => check(
            "contract",
            "Contract",
            true,
            true,
            "valid",
            output.stdout.trim().to_string(),
        )
        .with_data(json!({ "exit_status": output.exit_status })),
        Ok(output) => check(
            "contract",
            "Contract",
            true,
            false,
            "invalid",
            output.stderr.trim().to_string(),
        )
        .with_fix("Run `scripts/jig check contract --no-receipt` for the full contract report.")
        .with_data(json!({
                "exit_status": output.exit_status,
                "stdout": output.stdout,
                "stderr": output.stderr,
        })),
        Err(error) => check(
            "contract",
            "Contract",
            true,
            false,
            "error",
            error.to_string(),
        )
        .with_fix("Run `scripts/jig check contract --no-receipt` for the full contract report."),
    }
}

fn required_tools_check(ctx: &RepoContext) -> DoctorCheck {
    let mut tools = Vec::new();
    let mut missing = Vec::new();
    let mut executable_count = 0;
    for command_key in ctx.required_commands() {
        let command = match ctx.command_for_key(command_key) {
            Ok(command) => command,
            Err(error) => {
                missing.push(format!("{command_key}: {error}"));
                tools.push(json!({
                    "command_key": command_key,
                    "command": null,
                    "program": null,
                    "present": false,
                    "detail": error.to_string(),
                }));
                continue;
            }
        };
        let programs = command_programs(ctx.root(), command);
        executable_count += programs.len();
        let probed_programs = if programs.is_empty() {
            vec![json!({
                "program": null,
                "present": true,
                "detail": "No external executable required.",
            })]
        } else {
            programs
                .iter()
                .map(|program| {
                    let (present, detail) = program_present(ctx.root(), program);
                    if !present {
                        missing.push(format!("{command_key}: {program}"));
                    }
                    json!({
                        "program": program,
                        "present": present,
                        "detail": detail,
                    })
                })
                .collect()
        };
        let all_present = probed_programs
            .iter()
            .all(|program| program["present"].as_bool().unwrap_or(false));
        tools.push(json!({
            "command_key": command_key,
            "command": command,
            "programs": probed_programs,
            "present": all_present,
        }));
    }

    let ok = missing.is_empty();
    check(
        "required_tools",
        "Required tools",
        true,
        ok,
        if ok { "present" } else { "missing" },
        if ok {
            format!(
                "{} required command(s) checked; {} external executable(s) found",
                tools.len(),
                executable_count
            )
        } else {
            format!("Missing command executable(s): {}", missing.join(", "))
        },
    )
    .with_optional_fix((!ok).then_some("Install the missing executable or restore the missing repo script, then run `scripts/jig doctor --summary`."))
    .with_data(json!({ "tools": tools }))
}

fn agent_check(ctx: &RepoContext) -> DoctorCheck {
    match crate::runtime::call_tool(ctx, tool::AGENT_DOCTOR, json!({})) {
        Ok(output) => {
            let ok = output["ok"].as_bool().unwrap_or(false);
            let configured = output["marketplaces"].as_array().map(Vec::len).unwrap_or(0);
            let registered = output["marketplaces"]
                .as_array()
                .map(|marketplaces| {
                    marketplaces
                        .iter()
                        .filter(|marketplace| marketplace["registered"].as_bool().unwrap_or(false))
                        .count()
                })
                .unwrap_or(0);
            let detail = if configured == 0 {
                "no agent skill marketplaces configured".into()
            } else {
                format!("{registered}/{configured} configured marketplace(s) registered")
            };
            let fix = output["next_steps"]
                .as_array()
                .and_then(|steps| agent_next_step(steps))
                .map(str::to_string);
            check(
                "agent_skills",
                "Agent skills",
                true,
                ok,
                if ok { "installed" } else { "missing" },
                detail,
            )
            .with_optional_fix(fix.as_deref())
            .with_data(output)
        }
        Err(error) => check(
            "agent_skills",
            "Agent skills",
            true,
            false,
            "error",
            error.to_string(),
        )
        .with_fix("Run `scripts/jig agent doctor --summary` for agent tooling details."),
    }
}

fn agent_next_step(steps: &[Value]) -> Option<&str> {
    steps
        .iter()
        .filter_map(Value::as_str)
        .find(|step| step.contains("`scripts/jig "))
        .or_else(|| steps.iter().filter_map(Value::as_str).next())
}

fn proxy_check(ctx: &RepoContext) -> DoctorCheck {
    let configured = !ctx.frontend_apps().is_empty()
        || !ctx.dev_config().apps.is_empty()
        || ctx.dev_config().workspace_discovery;
    if !configured {
        return check(
            "proxy",
            "Proxy",
            false,
            true,
            "not configured",
            "no dev apps configured",
        )
        .with_data(json!({ "configured": false }));
    }

    match proxy_list_output(ctx) {
        Ok(output) => proxy_check_from_output(configured, output),
        Err(error) => check("proxy", "Proxy", false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig proxy list` for proxy diagnostics.")
            .with_data(json!({ "configured": configured })),
    }
}

#[cfg(feature = "dev-proxy")]
fn proxy_list_output(ctx: &RepoContext) -> Result<Value> {
    crate::dev_proxy::commands::proxy(
        ctx,
        ProxyCommand::List(ProxyListRequest {
            raw: false,
            proxy: ProxyRuntimeOptions::default(),
        }),
    )
}

#[cfg(not(feature = "dev-proxy"))]
fn proxy_list_output(ctx: &RepoContext) -> Result<Value> {
    let launcher = ctx.root().join("scripts/jig");
    let output = Command::new(&launcher)
        .args(["proxy", "list"])
        .current_dir(ctx.root())
        .output()
        .with_context(|| {
            format!(
                "Failed to run proxy diagnostics through {}",
                launcher.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(anyhow!(
            "`scripts/jig proxy list` exited with status {}{}",
            output.status,
            if detail.is_empty() {
                String::new()
            } else {
                format!(": {detail}")
            }
        ));
    }

    serde_json::from_slice(&output.stdout).context("Failed to parse `scripts/jig proxy list` JSON")
}

fn proxy_check_from_output(configured: bool, output: Value) -> DoctorCheck {
    let running = output["running"].as_bool().unwrap_or(false);
    let status = match (configured, running) {
        (true, true) => "running",
        (true, false) => "not running",
        (false, true) => "running unconfigured",
        (false, false) => "not configured",
    };
    check(
        "proxy",
        "Proxy",
        false,
        running,
        status,
        proxy_detail(configured, running, &output),
    )
    .with_optional_fix((configured && !running).then_some("Run `scripts/jig proxy start`."))
    .with_data(json!({
            "configured": configured,
            "status": output,
    }))
}

fn proxy_detail(configured: bool, running: bool, output: &Value) -> String {
    let state_dir = output["state_dir"].as_str().unwrap_or("<unknown>");
    match (configured, running) {
        (true, true) => format!("configured and running; state_dir={state_dir}"),
        (true, false) => format!("configured but not running; state_dir={state_dir}"),
        (false, true) => format!("running, but no dev apps are configured; state_dir={state_dir}"),
        (false, false) => format!("no dev apps configured; state_dir={state_dir}"),
    }
}

fn vault_check() -> DoctorCheck {
    // Vault status is intentionally a cheap metadata probe and must not prompt
    // for a passphrase; doctor relies on that non-authenticated boundary.
    match crate::runtime::dispatch_vault(VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions::default(),
    })) {
        Ok(output) => {
            let initialized = output["exists"].as_bool().unwrap_or(false);
            check(
                "vault",
                "Vault",
                false,
                initialized,
                if initialized {
                    "initialized"
                } else {
                    "not initialized"
                },
                format!(
                    "vault_home={}",
                    output["vault_home"].as_str().unwrap_or("<unknown>")
                ),
            )
            .with_optional_fix((!initialized).then_some("Run `scripts/jig vault init`."))
            .with_data(output)
        }
        Err(error) => check("vault", "Vault", false, false, "error", error.to_string())
            .with_fix("Run `scripts/jig vault status` for vault diagnostics."),
    }
}

#[derive(Clone, Debug, Serialize)]
struct DoctorCheck {
    id: String,
    label: String,
    required: bool,
    ok: bool,
    status: String,
    detail: String,
    fix: Option<String>,
    data: Value,
}

fn check(
    id: &str,
    label: &str,
    required: bool,
    ok: bool,
    status: &str,
    detail: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id: id.to_string(),
        label: label.to_string(),
        required,
        ok,
        status: status.to_string(),
        detail: detail.into(),
        fix: None,
        data: json!({}),
    }
}

impl DoctorCheck {
    fn with_fix(mut self, fix: &str) -> Self {
        self.fix = Some(fix.to_string());
        self
    }

    fn with_optional_fix(mut self, fix: Option<&str>) -> Self {
        self.fix = fix.map(str::to_string);
        self
    }

    fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

fn command_programs(root: &Path, command: &str) -> Vec<String> {
    if let Some(branch) = active_optional_cargo_branch(root, command) {
        return command_programs_for_shell(&branch);
    }
    command_programs_for_shell(command)
}

fn active_optional_cargo_branch(root: &Path, command: &str) -> Option<String> {
    let (then_branch, else_branch) = crate::shell::optional_cargo_command_branches(command)?;
    Some(
        if root.join("Cargo.toml").exists() {
            then_branch
        } else {
            else_branch
        }
        .to_string(),
    )
}

#[cfg(test)]
fn command_program(command: &str) -> Option<String> {
    // Best-effort shell token recognition for diagnostics only. Runtime command
    // execution still goes through the configured shell command unchanged.
    command_programs_for_shell(command).into_iter().next()
}

fn command_programs_for_shell(command: &str) -> Vec<String> {
    shell_simple_commands(command)
        .iter()
        .filter_map(|words| command_program_for_words(words))
        .collect()
}

fn command_program_for_words(words: &[String]) -> Option<String> {
    let mut index = 0;
    while let Some(word) = words.get(index) {
        let word = trim_shell_quotes(word);
        if word.is_empty()
            || shell_command_prefix_keyword(&word)
            || shell_command_wrapper(&word)
            || looks_like_shell_assignment(&word)
        {
            index += 1;
            continue;
        }
        if word == "env" {
            index = env_program_index(words, index + 1)?;
            continue;
        }
        if shell_builtin_or_keyword(&word) {
            return None;
        }
        return Some(word);
    }
    None
}

fn env_program_index(words: &[String], mut index: usize) -> Option<usize> {
    let mut skip_option_arg = false;
    while let Some(word) = words.get(index) {
        let word = trim_shell_quotes(word);
        if word.is_empty() {
            index += 1;
            continue;
        }
        if skip_option_arg {
            skip_option_arg = false;
            index += 1;
            continue;
        }
        if looks_like_shell_assignment(&word) || word == "--" {
            index += 1;
            continue;
        }
        if word.starts_with('-') {
            skip_option_arg = env_option_takes_value(&word);
            index += 1;
            continue;
        }
        return Some(index);
    }
    None
}

fn env_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "-u" | "--unset" | "-C" | "--chdir" | "-S" | "--split-string"
    ) && !option.contains('=')
}

fn shell_simple_commands(command: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    let mut skip_next_word = false;

    for token in shell_tokens(command) {
        match token {
            ShellToken::Word(word) => {
                if skip_next_word {
                    skip_next_word = false;
                } else {
                    current.push(word);
                }
            }
            ShellToken::Redirection(redirection) => {
                skip_next_word = !redirection_has_inline_target(&redirection);
            }
            ShellToken::Separator => {
                if !current.is_empty() {
                    commands.push(std::mem::take(&mut current));
                }
                skip_next_word = false;
            }
        }
    }

    if !current.is_empty() {
        commands.push(current);
    }
    commands
}

#[derive(Debug, Eq, PartialEq)]
enum ShellToken {
    Word(String),
    Separator,
    Redirection(String),
}

fn shell_tokens(command: &str) -> Vec<ShellToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(quote_ch) = quote {
            if ch == quote_ch {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if ch.is_whitespace() => push_shell_word(&mut tokens, &mut current),
            ';' | '(' | ')' => {
                push_shell_word(&mut tokens, &mut current);
                tokens.push(ShellToken::Separator);
            }
            '&' | '|' => {
                push_shell_word(&mut tokens, &mut current);
                if chars.peek() == Some(&ch) {
                    chars.next();
                }
                tokens.push(ShellToken::Separator);
            }
            '<' | '>' => {
                push_shell_word_or_drop_fd_prefix(&mut tokens, &mut current);
                let mut redirection = String::from(ch);
                if chars.peek() == Some(&ch) {
                    redirection.push(chars.next().expect("peeked redirection operator"));
                }
                if chars.peek() == Some(&'&') {
                    redirection.push(chars.next().expect("peeked redirection target marker"));
                }
                while let Some(next) = chars.peek().copied() {
                    if next.is_whitespace()
                        || is_shell_separator_char(next)
                        || matches!(next, '<' | '>')
                    {
                        break;
                    }
                    redirection.push(chars.next().expect("peeked redirection target"));
                }
                tokens.push(ShellToken::Redirection(redirection));
            }
            _ => current.push(ch),
        }
    }

    push_shell_word(&mut tokens, &mut current);
    tokens
}

fn push_shell_word(tokens: &mut Vec<ShellToken>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(ShellToken::Word(std::mem::take(current)));
    }
}

fn push_shell_word_or_drop_fd_prefix(tokens: &mut Vec<ShellToken>, current: &mut String) {
    if !current.is_empty() && current.chars().all(|ch| ch.is_ascii_digit()) {
        current.clear();
    } else {
        push_shell_word(tokens, current);
    }
}

fn redirection_has_inline_target(redirection: &str) -> bool {
    let operator_len = if redirection.starts_with(">>") || redirection.starts_with("<<") {
        2
    } else {
        1
    };
    redirection.len() > operator_len
}

fn is_shell_separator_char(ch: char) -> bool {
    matches!(ch, ';' | '&' | '|' | '(' | ')')
}

fn looks_like_shell_assignment(value: &str) -> bool {
    let Some((name, _)) = value.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn shell_command_prefix_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "do" | "elif" | "else" | "if" | "then" | "until" | "while"
    )
}

fn shell_command_wrapper(program: &str) -> bool {
    matches!(program, "command" | "exec" | "nohup")
}

fn shell_builtin_or_keyword(program: &str) -> bool {
    matches!(
        program,
        "!" | "."
            | ":"
            | "["
            | "alias"
            | "bg"
            | "break"
            | "case"
            | "cd"
            | "command"
            | "continue"
            | "echo"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fg"
            | "for"
            | "function"
            | "if"
            | "jobs"
            | "local"
            | "printf"
            | "pwd"
            | "read"
            | "readonly"
            | "return"
            | "set"
            | "shift"
            | "test"
            | "then"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "until"
            | "while"
    )
}

fn trim_shell_quotes(value: &str) -> String {
    value
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(';')
        .to_string()
}

fn program_present(root: &Path, program: &str) -> (bool, String) {
    if program.contains('/') {
        let path = PathBuf::from(program);
        let path = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        return if executable_exists(&path) {
            (true, path.display().to_string())
        } else {
            (
                false,
                format!("{} is missing or not executable", path.display()),
            )
        };
    }

    for dir in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        let path = dir.join(program);
        if executable_exists(&path) {
            return (true, path.display().to_string());
        }
    }
    (false, format!("{program} was not found on PATH"))
}

fn executable_exists(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_env::lock_env;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn command_program_skips_env_and_assignments() {
        assert_eq!(command_program("cargo test").as_deref(), Some("cargo"));
        assert_eq!(
            command_program("RUSTFLAGS=-Dwarnings cargo test").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            command_program("env RUSTFLAGS=-Dwarnings cargo test").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            command_program("\"scripts/jig\" check contract").as_deref(),
            Some("scripts/jig")
        );
    }

    #[test]
    fn command_programs_report_compound_command_executables() {
        assert_eq!(
            command_programs(Path::new("."), "cargo test && npm run build"),
            vec!["cargo", "npm"]
        );
        assert_eq!(
            command_programs(
                Path::new("."),
                "RUSTFLAGS=-Dwarnings cargo test; env NODE_ENV=test pnpm test"
            ),
            vec!["cargo", "pnpm"]
        );
    }

    #[test]
    fn command_programs_skip_builtins_and_redirection_targets() {
        assert_eq!(
            command_programs(
                Path::new("."),
                "printf '%s\\n' skipped > /tmp/out && cargo test 2>&1"
            ),
            vec!["cargo"]
        );
    }

    #[test]
    fn command_programs_follow_generated_optional_cargo_branch() {
        let temp = tempdir().unwrap();
        let command = format!(
            "{}cargo fetch{}printf '%s\\n' skipped{}",
            crate::shell::OPTIONAL_CARGO_COMMAND_PREFIX,
            crate::shell::OPTIONAL_CARGO_COMMAND_ELSE,
            crate::shell::OPTIONAL_CARGO_COMMAND_SUFFIX,
        );

        assert!(command_programs(temp.path(), &command).is_empty());

        fs::write(temp.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(command_programs(temp.path(), &command), vec!["cargo"]);
    }

    #[test]
    fn runtime_check_accepts_launcher_without_readable_pin_when_config_matches() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("scripts")).unwrap();
        fs::write(temp.path().join("scripts/jig"), "#!/usr/bin/env bash\n").unwrap();

        let output = runtime_check(temp.path(), Some(env!("CARGO_PKG_VERSION")));

        assert!(output.ok);
        assert_eq!(output.status, "unverified launcher");
        assert!(output.fix.is_none());
    }

    #[test]
    fn runtime_check_reports_unreadable_launcher() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("scripts/jig")).unwrap();

        let output = runtime_check(temp.path(), Some(env!("CARGO_PKG_VERSION")));

        assert!(!output.ok);
        assert_eq!(output.status, "mismatch");
        assert!(output.detail.contains("unreadable"));
        assert!(output.data["launcher_error"].as_str().is_some());
        assert!(output.fix.as_deref().unwrap().contains("jig update"));
    }

    #[test]
    fn agent_next_step_prefers_command_shaped_steps() {
        let steps = vec![
            json!("Codex CLI is not available on PATH."),
            json!("Run `scripts/jig agent bootstrap` to register skills."),
        ];

        assert_eq!(
            agent_next_step(&steps),
            Some("Run `scripts/jig agent bootstrap` to register skills.")
        );
    }

    #[test]
    fn doctor_reports_unified_readiness_checks() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        write_doctor_fixture(temp.path());
        let original = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        let output = run().unwrap();

        env::set_current_dir(original).unwrap();
        assert_eq!(output["command"], "doctor");
        assert_eq!(output["repo"]["name"], "demo");
        assert_eq!(output["checks"].as_array().unwrap().len(), 8);
        assert!(check_by_id(&output, "runtime")["ok"].as_bool().unwrap());
        assert!(check_by_id(&output, "config")["ok"].as_bool().unwrap());
        assert!(check_by_id(&output, "contract")["ok"].as_bool().unwrap());
        assert!(
            check_by_id(&output, "required_tools")["ok"]
                .as_bool()
                .unwrap()
        );
        assert!(
            check_by_id(&output, "agent_skills")["ok"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(check_by_id(&output, "proxy")["status"], "not configured");
        assert!(check_by_id(&output, "proxy")["ok"].as_bool().unwrap());
        assert_eq!(check_by_id(&output, "vault")["required"], false);
    }

    #[test]
    fn doctor_reports_all_checks_when_config_is_invalid() {
        let _env = lock_env();
        let temp = tempdir().unwrap();
        fs::write(temp.path().join(".jig.toml"), "repo_name = \n").unwrap();
        fs::create_dir_all(temp.path().join("scripts")).unwrap();
        fs::write(
            temp.path().join("scripts/jig"),
            "#!/usr/bin/env bash\nJIG_VERSION=\"0.2.0-beta.1\"\n",
        )
        .unwrap();
        let original = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        let output = run().unwrap();

        env::set_current_dir(original).unwrap();
        assert_eq!(output["command"], "doctor");
        assert_eq!(output["checks"].as_array().unwrap().len(), 8);
        assert_eq!(check_by_id(&output, "config")["status"], "invalid");
        assert_eq!(check_by_id(&output, "contract")["status"], "blocked");
        assert_eq!(check_by_id(&output, "required_tools")["status"], "blocked");
        assert_eq!(check_by_id(&output, "agent_skills")["status"], "blocked");
        assert_eq!(check_by_id(&output, "proxy")["status"], "blocked");
        for id in ["contract", "required_tools", "agent_skills", "proxy"] {
            assert!(
                check_by_id(&output, id)["detail"]
                    .as_str()
                    .unwrap()
                    .contains(".jig.toml")
            );
        }
        assert!(output["next_step"].as_str().unwrap().contains(".jig.toml"));
    }

    fn check_by_id<'a>(output: &'a Value, id: &str) -> &'a Value {
        output["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["id"] == id)
            .unwrap()
    }

    fn write_doctor_fixture(root: &Path) {
        fs::create_dir_all(root.join(".agent")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join(".jig.toml"),
            r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "printf bootstrap"

[agent_tooling.codex]
marketplaces = []
"#,
        )
        .unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 3,
                "tool_namespace": "jig",
                "jig_version": "0.2.0-beta.1",
                "required_commands": ["bootstrap_command"],
                "tools": [
                    {
                        "name": tool::CONTRACT_CHECK,
                        "kind": "native",
                        "description": "Contract check."
                    },
                    {
                        "name": tool::BOOTSTRAP,
                        "kind": "command",
                        "description": "Bootstrap.",
                        "command": "bootstrap_command"
                    }
                ],
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join(".mcp.json"), "{}").unwrap();
        fs::write(root.join("scripts/install-jig.sh"), "#!/usr/bin/env bash\n").unwrap();
        fs::write(
            root.join("scripts/jig"),
            "#!/usr/bin/env bash\nJIG_VERSION=\"0.2.0-beta.1\"\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join("scripts/jig"), fs::Permissions::from_mode(0o755))
                .unwrap();
        }
    }
}
