#[cfg(test)]
use std::io::BufRead;
use std::io::{self, IsTerminal, Write};
use std::process;

use anyhow::{Context, Result};
use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};
use rustyline::{DefaultEditor, error::ReadlineError};
use serde_json::{Value, json};

use super::output::{HumanOutput, print_json, print_output};
#[cfg(test)]
use super::output::{
    format_agent_doctor_summary, format_vault_run_summary, format_work_check_summary,
    format_work_evidence_summary, format_work_gates_summary, format_work_receipts_summary,
    format_work_start_plan_id, format_work_status_summary,
};
use super::prompt::PromptAddOpts;
use super::*;

fn attach_bootstrap_vault(output: &mut Value, vault: Value) -> Result<()> {
    if output.get("vault").is_some() {
        anyhow::bail!("bootstrap output unexpectedly included a vault field");
    }
    output["vault"] = vault;
    Ok(())
}

#[derive(Debug)]
struct JsonOkFalse;

#[derive(Debug)]
struct VaultChildExitStatus(i32);

impl std::fmt::Display for JsonOkFalse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command reported ok=false")
    }
}

impl std::error::Error for JsonOkFalse {}

impl std::fmt::Display for VaultChildExitStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Vault child exited with status {}", self.0)
    }
}

impl std::error::Error for VaultChildExitStatus {}

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
    let json_output = cli.json;
    match cli.command {
        CommandKind::Init(opts) => {
            let vault_setup =
                prepare_bootstrap_vault(!opts.no_vault, opts.no_input, opts.defaults)?;
            let mut output = bootstrap::run_init(opts)?;
            let vault =
                ensure_bootstrap_vault(&output, vault_setup.requested, !vault_setup.pre_captured)?;
            attach_bootstrap_vault(&mut output, vault)?;
            if json_output {
                print_json(&output)?;
            } else {
                print_init_human_summary(&output)?;
            }
            Ok(())
        }
        CommandKind::Presets => {
            let output = bootstrap::scaffold_presets_report();
            if json_output {
                print_json(&output)?;
            } else {
                print_presets_human_summary(&output)?;
            }
            Ok(())
        }
        CommandKind::Adopt(opts) => {
            let vault_setup = prepare_bootstrap_vault(
                opts.write && !opts.no_vault,
                opts.no_input,
                opts.defaults,
            )?;
            let mut output = bootstrap::run_adopt(opts)?;
            let vault =
                ensure_bootstrap_vault(&output, vault_setup.requested, !vault_setup.pre_captured)?;
            attach_bootstrap_vault(&mut output, vault)?;
            if json_output {
                print_json(&output)?;
            } else {
                print_adopt_human_summary(&output)?;
            }
            Ok(())
        }
        CommandKind::Update(opts) => {
            let output = bootstrap::run_update(opts)?;
            if json_output {
                print_json(&output)?;
            } else {
                print_update_human_summary(&output)?;
            }
            Ok(())
        }
        CommandKind::Mcp => {
            let ctx = RepoContext::load()?;
            mcp::serve(&ctx)
        }
        CommandKind::Doctor(opts) => {
            let output = doctor::run()?;
            print_output(opts.summary.then_some(HumanOutput::DoctorSummary), &output)?;
            require_json_ok(true, &output)
        }
        CommandKind::Info(opts) => {
            let output = info::run()?;
            print_output(opts.summary.then_some(HumanOutput::InfoSummary), &output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Dev(opts) => {
            let output = crate::dev_proxy::commands::dev_without_context(opts.into())?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Dev(opts) => {
            let Some(ctx) = RepoContext::load_optional()? else {
                anyhow::bail!(
                    "`scripts/jig dev` requires an adopted Jig repo with `.jig.toml` dev app configuration. Run it from a Jig repo, or use `scripts/jig proxy run <name> -- <command>` for an ad-hoc command."
                );
            };
            let output = runtime::dispatch(&ctx, crate::command::RuntimeCommand::Dev(opts.into()))?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Proxy(command) => {
            let output = crate::dev_proxy::commands::proxy_without_context(command.into())?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Proxy(command) => {
            let runtime_command: crate::command::ProxyCommand = command.into();
            let output = if crate::dev_proxy::commands::can_run_without_context(&runtime_command) {
                if let Some(ctx) = RepoContext::load_optional()? {
                    runtime::dispatch(&ctx, crate::command::RuntimeCommand::Proxy(runtime_command))?
                } else {
                    crate::dev_proxy::commands::proxy_without_context(runtime_command)?
                }
            } else {
                let ctx = RepoContext::load()?;
                runtime::dispatch(&ctx, crate::command::RuntimeCommand::Proxy(runtime_command))?
            };
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        CommandKind::Bootstrap(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::Bootstrap(opts.into()),
            false,
            None,
        ),
        CommandKind::Check(command) => {
            let require_ok = check_command_reports_failure_with_ok(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Check(command.into()),
                require_ok,
                None,
            )
        }
        CommandKind::SchemaDump(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::SchemaDump(opts.into()),
            false,
            None,
        ),
        CommandKind::MigrationAdd(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::MigrationAdd(opts.into()),
            false,
            None,
        ),
        CommandKind::AgentMap(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::AgentMap(command.into()),
            false,
            None,
        ),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts.into()),
            false,
            None,
        ),
        CommandKind::Vault(command) => {
            let vault_run_summary = matches!(&command, VaultCommand::Run(opts) if opts.summary);
            let mut runtime_command: crate::command::VaultCommand = command.into();
            apply_repo_vault_scope(&mut runtime_command)?;
            let is_run = matches!(runtime_command, crate::command::VaultCommand::Run(_));
            if vault_command_requires_passphrase(&runtime_command) {
                // Invariant: capture and clear the process environment copy
                // before vault runtime code can start background threads.
                if matches!(runtime_command, crate::command::VaultCommand::Init(_)) {
                    runtime::capture_new_vault_passphrase()?;
                } else {
                    runtime::capture_vault_passphrase()?;
                }
            }
            let output = runtime::dispatch_vault(runtime_command)?;
            print_output(
                vault_run_summary.then_some(HumanOutput::VaultRunSummary),
                &output,
            )?;
            if is_run {
                // `vault run` mirrors the child process status. Its JSON `ok`
                // field is derived from that same status, so avoid reporting a
                // second generic ok=false error for the same child failure.
                return require_vault_child_status_ok(&output);
            }
            require_json_ok(true, &output)
        }
        CommandKind::Prompt(command) => run_prompt_command(command, json_output),
        CommandKind::Agent(command) => {
            let require_ok = agent_command_reports_failure_with_ok(&command);
            let human_output = agent_human_output_requested(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Agent(command.into()),
                require_ok,
                human_output,
            )
        }
        CommandKind::Work(command) => {
            let human_output = work_human_output_requested(&command);
            dispatch_runtime_command(
                crate::command::RuntimeCommand::Work(command.into()),
                false,
                human_output,
            )
        }
        CommandKind::State(command) => dispatch_runtime_command(
            crate::command::RuntimeCommand::State(command.into()),
            false,
            None,
        ),
    }
}

fn run_prompt_command(command: PromptCommand, json_output: bool) -> Result<()> {
    let repo = RepoContext::load_optional()?;
    let registry =
        crate::prompt_registry::PromptRegistry::from_env(repo.as_ref().map(|ctx| ctx.root()))?;
    match command {
        PromptCommand::Get(opts) => {
            let body = registry.render_prompt(crate::prompt_registry::PromptRenderRequest {
                name: opts.name,
                vars: opts.vars,
                raw: opts.raw,
            })?;
            // `prompt get` is the raw prompt primitive: stdout is exactly the
            // rendered body even when the global --json flag is present.
            let mut stdout = io::stdout().lock();
            stdout.write_all(body.as_bytes())?;
            stdout.flush()?;
            Ok(())
        }
        PromptCommand::Copy(opts) => print_prompt_output(
            registry.copy_prompt(crate::prompt_registry::PromptRenderRequest {
                name: opts.name,
                vars: opts.vars,
                raw: opts.raw,
            })?,
            json_output,
        ),
        PromptCommand::Add(opts) => print_prompt_output(
            if prompt_add_uses_editor(&opts) {
                registry.add_prompt_with_editor(prompt_add_request(opts)?)
            } else {
                registry.add_prompt(prompt_add_request(opts)?)
            }?,
            json_output,
        ),
        PromptCommand::Edit(opts) => print_prompt_output(
            if opts.no_editor {
                registry.prompt_edit_target(&opts.name)?
            } else {
                registry.edit_prompt(&opts.name)?
            },
            json_output,
        ),
        PromptCommand::Remove(opts) => {
            print_prompt_output(registry.remove_prompt(&opts.name)?, json_output)
        }
        PromptCommand::List(opts) => {
            print_prompt_output(registry.list_prompts(!opts.no_packs)?, json_output)
        }
        PromptCommand::Search(opts) => print_prompt_output(
            registry.search_prompts(&opts.query, opts.body)?,
            json_output,
        ),
        PromptCommand::Export(opts) => {
            let archive = registry.export_prompts()?;
            if let Some(output) = opts.output {
                let text = serde_json::to_string_pretty(&archive)?;
                crate::prompt_registry::write_bytes_atomic(
                    &output,
                    format!("{text}\n").as_bytes(),
                )?;
                let result = json!({
                    "ok": true,
                    "command": "prompt export",
                    "output": output,
                    "prompt_count": archive["prompts"].as_array().map(Vec::len).unwrap_or(0),
                });
                print_prompt_output(result, json_output)
            } else if json_output {
                print_json(&json!({
                    "ok": true,
                    "command": "prompt export",
                    "prompt_count": archive["prompts"].as_array().map(Vec::len).unwrap_or(0),
                    "archive": archive,
                }))
            } else {
                // Without --output the archive itself is the requested artifact.
                print_json(&archive)
            }
        }
        PromptCommand::Import(opts) => {
            print_prompt_output(registry.import_prompts(&opts.file)?, json_output)
        }
    }
}

fn prompt_add_request(opts: PromptAddOpts) -> Result<crate::prompt_registry::PromptAddRequest> {
    if prompt_add_needs_interaction(&opts) {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            anyhow::bail!(
                "prompt add needs interactive input; pass NAME plus BODY or --file, or omit --no-editor to use $VISUAL or $EDITOR"
            );
        }
        interactive_prompt_add_request_terminal(opts)
    } else {
        Ok(crate::prompt_registry::PromptAddRequest {
            name: opts.name.expect("checked by prompt_add_needs_interaction"),
            body: opts.body,
            file: opts.file,
            description: opts.description,
            tags: opts.tags,
        })
    }
}

fn prompt_add_needs_interaction(opts: &PromptAddOpts) -> bool {
    opts.name.is_none() || (opts.no_editor && opts.body.is_none() && opts.file.is_none())
}

fn prompt_add_uses_editor(opts: &PromptAddOpts) -> bool {
    opts.name.is_some() && opts.body.is_none() && opts.file.is_none() && !opts.no_editor
}

fn interactive_prompt_add_request_terminal(
    opts: PromptAddOpts,
) -> Result<crate::prompt_registry::PromptAddRequest> {
    let mut output = io::stderr();
    writeln!(output, "Interactive prompt add")?;
    let mut editor = DefaultEditor::new().context("Failed to initialize prompt line editor")?;
    let name = match opts.name {
        Some(name) => name,
        None => prompt_required_line_editor(&mut editor, "Prompt name: ", "prompt name")?,
    };
    let description = match opts.description {
        Some(description) => Some(description),
        None => {
            let value = prompt_optional_line_editor(&mut editor, "Description (optional): ")?;
            if value.is_empty() { None } else { Some(value) }
        }
    };
    let tags = if opts.tags.is_empty() {
        parse_interactive_tags(&prompt_optional_line_editor(
            &mut editor,
            "Tags (comma-separated, optional): ",
        )?)
    } else {
        opts.tags
    };
    let body = if opts.body.is_none() && opts.file.is_none() {
        Some(prompt_body_line_editor(&mut editor, &mut output)?)
    } else {
        opts.body
    };
    Ok(crate::prompt_registry::PromptAddRequest {
        name,
        body,
        file: opts.file,
        description,
        tags,
    })
}

#[cfg(test)]
fn interactive_prompt_add_request<R: BufRead, W: Write>(
    opts: PromptAddOpts,
    mut input: R,
    output: &mut W,
) -> Result<crate::prompt_registry::PromptAddRequest> {
    writeln!(output, "Interactive prompt add")?;
    let name = match opts.name {
        Some(name) => name,
        None => prompt_required_line(&mut input, output, "Prompt name: ", "prompt name")?,
    };
    let description = match opts.description {
        Some(description) => Some(description),
        None => {
            let value = prompt_optional_line(&mut input, output, "Description (optional): ")?;
            if value.is_empty() { None } else { Some(value) }
        }
    };
    let tags = if opts.tags.is_empty() {
        parse_interactive_tags(&prompt_optional_line(
            &mut input,
            output,
            "Tags (comma-separated, optional): ",
        )?)
    } else {
        opts.tags
    };
    let body = if opts.body.is_none() && opts.file.is_none() {
        Some(prompt_body(&mut input, output)?)
    } else {
        opts.body
    };
    Ok(crate::prompt_registry::PromptAddRequest {
        name,
        body,
        file: opts.file,
        description,
        tags,
    })
}

fn prompt_required_line_editor(
    editor: &mut DefaultEditor,
    prompt: &str,
    label: &str,
) -> Result<String> {
    loop {
        let value = prompt_optional_line_editor(editor, prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }
        eprintln!("{label} cannot be empty");
    }
}

fn prompt_optional_line_editor(editor: &mut DefaultEditor, prompt: &str) -> Result<String> {
    match editor.readline(prompt) {
        Ok(line) => Ok(line),
        Err(ReadlineError::Interrupted) => anyhow::bail!("interactive prompt add interrupted"),
        Err(ReadlineError::Eof) => Ok(String::new()),
        Err(error) => Err(error).context("Failed to read interactive prompt input"),
    }
}

#[cfg(test)]
fn prompt_required_line<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
    label: &str,
) -> Result<String> {
    loop {
        let value = prompt_optional_line(input, output, prompt)?;
        if !value.is_empty() {
            return Ok(value);
        }
        writeln!(output, "{label} cannot be empty")?;
    }
}

#[cfg(test)]
fn prompt_optional_line<R: BufRead, W: Write>(
    input: &mut R,
    output: &mut W,
    prompt: &str,
) -> Result<String> {
    write!(output, "{prompt}")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        anyhow::bail!("interactive prompt add ended before input was complete");
    }
    Ok(trim_line_ending(line))
}

fn prompt_body_line_editor<W: Write>(editor: &mut DefaultEditor, output: &mut W) -> Result<String> {
    writeln!(
        output,
        "Prompt body. Finish with Ctrl-D or a line containing only a single dot."
    )?;
    let mut lines = Vec::new();
    loop {
        let line = match editor.readline("> ") {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => anyhow::bail!("interactive prompt add interrupted"),
            Err(ReadlineError::Eof) if lines.is_empty() => {
                anyhow::bail!("interactive prompt add ended before prompt body was complete")
            }
            Err(ReadlineError::Eof) => break,
            Err(error) => return Err(error).context("Failed to read interactive prompt body"),
        };
        if line == "." {
            break;
        }
        lines.push(line);
    }
    let body = lines.join("\n");
    if body.trim().is_empty() {
        anyhow::bail!("prompt body cannot be empty");
    }
    Ok(body)
}

#[cfg(test)]
fn prompt_body<R: BufRead, W: Write>(input: &mut R, output: &mut W) -> Result<String> {
    writeln!(
        output,
        "Prompt body. Finish with Ctrl-D or a line containing only a single dot."
    )?;
    let mut lines = Vec::new();
    loop {
        write!(output, "> ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 && lines.is_empty() {
            anyhow::bail!("interactive prompt add ended before prompt body was complete");
        } else if line.is_empty() {
            break;
        }
        let line = trim_line_ending(line);
        if line == "." {
            break;
        }
        lines.push(line);
    }
    let body = lines.join("\n");
    if body.trim().is_empty() {
        anyhow::bail!("prompt body cannot be empty");
    }
    Ok(body)
}

#[cfg(test)]
fn trim_line_ending(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

fn parse_interactive_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn print_prompt_output(output: serde_json::Value, json_output: bool) -> Result<()> {
    if json_output {
        print_json(&output)
    } else {
        crate::prompt_registry::print_prompt_warnings(&output);
        print_human_summary(crate::prompt_registry::format_prompt_human_output(&output)?)
    }
}

#[cfg(test)]
pub(super) fn test_command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    match command {
        CommandKind::Doctor(_) | CommandKind::Dev(_) | CommandKind::Proxy(_) => true,
        CommandKind::Vault(command) => matches!(command, VaultCommand::Run(_)),
        CommandKind::Agent(command) => agent_command_reports_failure_with_ok(command),
        CommandKind::Check(command) => check_command_reports_failure_with_ok(command),
        _ => false,
    }
}

fn vault_command_requires_passphrase(command: &crate::command::VaultCommand) -> bool {
    !matches!(command, crate::command::VaultCommand::Status(_))
}

#[derive(Clone, Copy, Debug)]
struct BootstrapVaultSetup {
    requested: bool,
    pre_captured: bool,
}

fn prepare_bootstrap_vault(
    requested: bool,
    no_input: bool,
    defaults: bool,
) -> Result<BootstrapVaultSetup> {
    let env_present = runtime::vault_passphrase_env_present();
    let pre_captured = should_pre_capture_bootstrap_vault(
        requested,
        no_input,
        defaults,
        env_present,
        runtime::vault_passphrase_prompt_available(),
    );
    reject_missing_no_input_vault_passphrase(requested, no_input, env_present)?;
    if pre_captured {
        runtime::capture_new_vault_passphrase()?;
    }
    Ok(BootstrapVaultSetup {
        requested,
        pre_captured,
    })
}

fn should_pre_capture_bootstrap_vault(
    requested: bool,
    no_input: bool,
    defaults: bool,
    env_present: bool,
    prompt_available: bool,
) -> bool {
    // `--defaults` is treated as automation intent for vault setup even though
    // it can still leave ordinary answer prompts interactive.
    requested && (no_input || defaults || env_present || !prompt_available)
}

fn reject_missing_no_input_vault_passphrase(
    requested: bool,
    no_input: bool,
    env_present: bool,
) -> Result<()> {
    if requested && no_input && !env_present {
        anyhow::bail!(
            "JIG_VAULT_PASSPHRASE is required when --no-input initializes a vault; export JIG_VAULT_PASSPHRASE or pass --no-vault to skip initial vault setup"
        );
    }
    Ok(())
}

fn apply_repo_vault_scope(command: &mut crate::command::VaultCommand) -> Result<()> {
    let options = vault_options_mut(command);
    if options.home.is_some() {
        return Ok(());
    }

    let Some(ctx) = RepoContext::load_optional()? else {
        return Ok(());
    };
    let vault = ctx.vault_config();
    apply_repo_vault_scope_to_options(
        options,
        runtime::repo_vault_options_for_context(&ctx),
        vault.allow_global(),
    )
}

fn vault_options_mut(
    command: &mut crate::command::VaultCommand,
) -> &mut crate::command::VaultRuntimeOptions {
    match command {
        crate::command::VaultCommand::Audit(command) => match command {
            crate::command::VaultAuditCommand::Verify(request) => &mut request.vault,
        },
        crate::command::VaultCommand::Init(request) => &mut request.vault,
        crate::command::VaultCommand::Status(request) => &mut request.vault,
        crate::command::VaultCommand::Secret(command) => match command {
            crate::command::VaultSecretCommand::List(request) => &mut request.vault,
            crate::command::VaultSecretCommand::Set(request) => &mut request.vault,
            crate::command::VaultSecretCommand::Remove(request) => &mut request.vault,
        },
        crate::command::VaultCommand::Run(request) => &mut request.vault,
    }
}

fn apply_repo_vault_scope_to_options(
    options: &mut crate::command::VaultRuntimeOptions,
    repo_options: Option<crate::command::VaultRuntimeOptions>,
    allow_global: bool,
) -> Result<()> {
    if options.home.is_some() {
        return Ok(());
    }
    let has_repo_scope = repo_options.is_some();
    match &options.scope {
        crate::command::VaultScopeSelection::Auto => {
            if let Some(repo_options) = repo_options {
                *options = repo_options;
            }
            Ok(())
        }
        crate::command::VaultScopeSelection::Global if !allow_global && has_repo_scope => {
            anyhow::bail!(
                "This repo is configured for repo-scoped vault access and [vault].allow_global is false; remove --global or set allow_global = true after reviewing the risk."
            )
        }
        _ => Ok(()),
    }
}

fn ensure_bootstrap_vault(
    output: &serde_json::Value,
    requested: bool,
    capture_passphrase: bool,
) -> Result<serde_json::Value> {
    let destination = output["destination"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("bootstrap output did not include destination"))?;
    if !requested {
        return Ok(json!({
            "requested": false,
            "initialized": false,
            "created": false,
            "skipped_reason": "disabled",
        }));
    }

    let ctx =
        RepoContext::load_from_root(std::path::PathBuf::from(destination)).with_context(|| {
            "vault auto-init could not load the rendered repo context after repo files were written; fix the reported .jig.toml or .agent/jig-contract.json issue before rerunning `jig vault init`"
        })?;
    let Some(vault) = runtime::repo_vault_options_for_context(&ctx) else {
        return Ok(json!({
            "requested": true,
            "initialized": false,
            "created": false,
            "skipped_reason": "repo has no [vault] scope",
        }));
    };
    let status = runtime::dispatch_vault(crate::command::VaultCommand::Status(
        crate::command::VaultStatusRequest {
            vault: vault.clone(),
        },
    ))
    .context("vault auto-init status check failed after repo files were written; rerun `jig vault status` from the repo after fixing the reported vault issue")?;
    if status["exists"].as_bool().unwrap_or(false) {
        return Ok(json!({
            "requested": true,
            "initialized": true,
            "created": false,
            "vault_home": status["vault_home"],
            "vault_scope": status["vault_scope"],
            "vault_scope_id": status["vault_scope_id"],
        }));
    }

    if capture_passphrase {
        runtime::capture_new_vault_passphrase().context(
            "vault auto-init passphrase capture failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue",
        )?;
    }
    let init = runtime::dispatch_vault(crate::command::VaultCommand::Init(
        crate::command::VaultInitRequest { vault },
    ))
    .context("vault auto-init failed after repo files were written; rerun `jig vault init` from the repo after fixing the reported vault issue")?;
    Ok(json!({
        "requested": true,
        "initialized": true,
        "created": true,
        "vault_home": init["vault_home"],
        "vault_scope": init["vault_scope"],
        "vault_scope_id": init["vault_scope_id"],
    }))
}

fn agent_command_reports_failure_with_ok(command: &AgentCommand) -> bool {
    matches!(command, AgentCommand::Doctor(_))
}

fn check_command_reports_failure_with_ok(command: &CheckCommand) -> bool {
    matches!(
        command,
        CheckCommand::AgentMap(_)
            | CheckCommand::AgentGuides
            | CheckCommand::RustFileLoc(_)
            | CheckCommand::NoModRs
            | CheckCommand::MigrationImmutability(_)
            | CheckCommand::SqlxUncheckedNonTest,
    )
}

fn agent_human_output_requested(command: &AgentCommand) -> Option<HumanOutput> {
    match command {
        AgentCommand::Doctor(opts) if opts.summary => Some(HumanOutput::AgentDoctorSummary),
        _ => None,
    }
}

fn work_human_output_requested(command: &WorkCommand) -> Option<HumanOutput> {
    match command {
        WorkCommand::Start(opts) if opts.print_plan_id => Some(HumanOutput::WorkStartPlanId),
        WorkCommand::Check(opts) if opts.summary => Some(HumanOutput::WorkCheckSummary),
        WorkCommand::Gates(opts) if opts.summary => Some(HumanOutput::WorkGatesSummary),
        WorkCommand::Evidence(opts) if opts.summary => Some(HumanOutput::WorkEvidenceSummary),
        WorkCommand::Review(opts) if opts.summary => Some(HumanOutput::WorkReviewSummary),
        WorkCommand::Refine(opts) if opts.summary => Some(HumanOutput::WorkRefineSummary),
        WorkCommand::Receipts(opts) if opts.summary => Some(HumanOutput::WorkReceiptsSummary),
        WorkCommand::Status(opts) if opts.summary => Some(HumanOutput::WorkStatusSummary),
        _ => None,
    }
}

fn dispatch_runtime_command(
    command: crate::command::RuntimeCommand,
    require_ok: bool,
    human_output: Option<HumanOutput>,
) -> Result<()> {
    let ctx = RepoContext::load()?;
    let output = runtime::dispatch(&ctx, command)?;
    print_output(human_output, &output)?;
    require_json_ok(require_ok, &output)
}

fn require_vault_child_status_ok(output: &serde_json::Value) -> Result<()> {
    let status = output
        .get("result")
        .and_then(|value| value.get("exit_status"))
        .and_then(serde_json::Value::as_i64);
    if status.is_none() && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        anyhow::bail!("vault run returned ok=false without result.exit_status");
    }
    let Some(status) = status else {
        return Ok(());
    };
    if status != 0 {
        // The CLI process exit API is limited to shell-style status bytes.
        // Preserve non-zero vault child failures while keeping output portable.
        return Err(VaultChildExitStatus(status.clamp(1, 255) as i32).into());
    }
    Ok(())
}

fn print_init_human_summary(output: &serde_json::Value) -> Result<()> {
    print_human_summary(format_init_human_summary(output))
}

fn print_presets_human_summary(output: &serde_json::Value) -> Result<()> {
    print_human_summary(format_presets_human_summary(output))
}

fn print_human_summary(summary: String) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(summary.as_bytes())?;
    Ok(())
}

pub(super) fn format_presets_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("available presets\n");
    let presets = output["presets"]
        .as_array()
        .expect("presets report must include a presets array");
    if presets.is_empty() {
        summary.push_str("  No presets are currently registered.\n");
        return summary;
    }
    for (index, preset) in presets.iter().enumerate() {
        if index > 0 {
            summary.push('\n');
        }
        let name = preset["name"].as_str().unwrap_or("<unknown>");
        let summary_text = preset["summary"].as_str().unwrap_or("");
        summary.push_str(&format!("  {name}\n"));
        if !summary_text.is_empty() {
            summary.push_str(&format!("    {summary_text}\n"));
        }
        if let Some(defaults) = preset["defaults"].as_array()
            && !defaults.is_empty()
        {
            summary.push_str("    defaults:\n");
            for default in defaults.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {default}\n"));
            }
        }
        if let Some(layout) = preset["layout"].as_array()
            && !layout.is_empty()
        {
            summary.push_str("    generated layout:\n");
            for path in layout.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {path}\n"));
            }
        }
        if let Some(frontends) = preset["frontend_shorthands"].as_array()
            && !frontends.is_empty()
        {
            summary.push_str("    frontend shorthands:\n");
            for frontend in frontends {
                let shorthand = frontend["name"].as_str().unwrap_or("<unknown>");
                let expands_to = frontend["expands_to"].as_str().unwrap_or("");
                summary.push_str(&format!("      - {shorthand}: {expands_to}\n"));
            }
        }
        if let Some(examples) = preset["examples"].as_array()
            && !examples.is_empty()
        {
            summary.push_str("    examples:\n");
            for example in examples.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      {example}\n"));
            }
        }
        if let Some(ownership) = preset["ownership"].as_str() {
            summary.push_str("    ownership:\n");
            summary.push_str(&format!("      - {ownership}\n"));
        }
        if let Some(non_goals) = preset["non_goals"].as_array()
            && !non_goals.is_empty()
        {
            summary.push_str("    non-goals:\n");
            for non_goal in non_goals.iter().filter_map(serde_json::Value::as_str) {
                summary.push_str(&format!("      - {non_goal}\n"));
            }
        }
    }
    summary
}

pub(super) fn format_init_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("init summary\n");
    push_summary_field(&mut summary, "target", output["destination"].as_str());
    push_summary_field(&mut summary, "template", output["template"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed\n"
    ));

    if let Some(scaffold) = output.get("scaffold").filter(|value| !value.is_null()) {
        let preset = scaffold["preset"].as_str().unwrap_or("<unknown>");
        let db = scaffold["db"].as_str().unwrap_or("<unknown>");
        summary.push_str(&format!("  scaffold: {preset}"));
        if let Some(repo_name) = scaffold["repo_name"].as_str() {
            summary.push_str(&format!(" for {repo_name}"));
        }
        summary.push_str(&format!(" (db: {db})\n"));
        let scaffold_created = array_len(&scaffold["files_created"]);
        let scaffold_modified = array_len(&scaffold["files_modified"]);
        let scaffold_unchanged = array_len(&scaffold["files_unchanged"]);
        summary.push_str(&format!(
            "  scaffold files: {scaffold_created} created, {scaffold_modified} modified, {scaffold_unchanged} unchanged\n"
        ));

        if let Some(frontends) = scaffold["frontends"].as_array()
            && !frontends.is_empty()
        {
            let names = frontends
                .iter()
                .filter_map(|frontend| frontend["name"].as_str())
                .collect::<Vec<_>>();
            if !names.is_empty() {
                summary.push_str(&format!("  frontends: {}\n", names.join(", ")));
            }
        }
    }

    if let Some(git_initialized) = output["git_initialized"].as_bool() {
        summary.push_str(&format!(
            "  git: {}\n",
            if git_initialized {
                "initialized"
            } else {
                "already present"
            }
        ));
    }

    push_vault_summary(&mut summary, &output["vault"]);

    if let Some(notes) = output["notes"].as_array()
        && !notes.is_empty()
    {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(5).filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {note}\n"));
        }
        if notes.len() > 5 {
            summary.push_str(&format!("    - and {} more\n", notes.len() - 5));
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {step}\n"));
        }
    }

    summary.push_str("  full report: rerun with --json\n");
    summary
}

fn print_adopt_human_summary(output: &serde_json::Value) -> Result<()> {
    print_human_summary(format_adopt_human_summary(output))
}

fn print_update_human_summary(output: &serde_json::Value) -> Result<()> {
    print_human_summary(format_update_human_summary(output))
}

pub(super) fn format_update_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("update summary\n");
    push_summary_field(&mut summary, "mode", output["render_mode"].as_str());
    push_summary_field(&mut summary, "target", output["destination"].as_str());
    push_summary_field(&mut summary, "answers", output["answers_file"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    let unchanged = array_len(&report["files_unchanged"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed, {unchanged} unchanged\n"
    ));

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        push_conflict_summary(&mut summary, "conflicts accepted", conflicts);
    }

    summary.push_str("  full report: rerun with --json\n");
    summary
}

pub(super) fn format_adopt_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("adopt summary\n");
    push_summary_field(&mut summary, "mode", output["render_mode"].as_str());
    push_summary_field(&mut summary, "target", output["destination"].as_str());

    let report = &output["render_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed\n"
    ));

    push_vault_summary(&mut summary, &output["vault"]);

    if let Some(review) = output["adoption_review"].as_array()
        && !review.is_empty()
    {
        summary.push_str("  review:\n");
        for item in review.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {item}\n"));
        }
    }

    if let Some(notes) = output["notes"].as_array()
        && !notes.is_empty()
    {
        summary.push_str("  notes:\n");
        for note in notes.iter().take(8).filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {note}\n"));
        }
        if notes.len() > 8 {
            summary.push_str(&format!("    - and {} more\n", notes.len() - 8));
        }
    }

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        push_conflict_summary(&mut summary, "conflicts", conflicts);
    }

    if let Some(warnings) = output["detection_report"]["warnings"].as_array()
        && !warnings.is_empty()
    {
        summary.push_str(&format!("  warnings: {}\n", warnings.len()));
        for warning in warnings
            .iter()
            .take(5)
            .filter_map(serde_json::Value::as_str)
        {
            summary.push_str(&format!("    - {warning}\n"));
        }
        if warnings.len() > 5 {
            summary.push_str(&format!("    - and {} more\n", warnings.len() - 5));
        }
    }

    if let Some(steps) = output["next_steps"].as_array()
        && !steps.is_empty()
    {
        summary.push_str("  next steps:\n");
        for step in steps.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {step}\n"));
        }
    }
    summary
}

fn push_conflict_summary(summary: &mut String, label: &str, conflicts: &[serde_json::Value]) {
    summary.push_str(&format!("  {label}: {}\n", conflicts.len()));
    for conflict in conflicts.iter().take(10) {
        let Some(path) = conflict["path"].as_str() else {
            continue;
        };
        if let Some(detail) = conflict["detail"].as_str() {
            summary.push_str(&format!("    - {path}: {detail}\n"));
        } else {
            summary.push_str(&format!("    - {path}\n"));
        }
    }
    if conflicts.len() > 10 {
        summary.push_str(&format!("    - and {} more\n", conflicts.len() - 10));
    }
}

fn push_vault_summary(summary: &mut String, vault: &serde_json::Value) {
    if vault.is_null() {
        return;
    }
    let requested = vault["requested"].as_bool().unwrap_or(false);
    if !requested {
        summary.push_str("  vault: skipped\n");
        return;
    }
    if let Some(reason) = vault["skipped_reason"].as_str() {
        summary.push_str(&format!("  vault: skipped ({reason})\n"));
        return;
    }
    let status = if vault["created"].as_bool().unwrap_or(false) {
        "created"
    } else if vault["initialized"].as_bool().unwrap_or(false) {
        "already initialized"
    } else {
        "not initialized"
    };
    summary.push_str(&format!("  vault: {status}"));
    if let Some(scope) = vault["vault_scope"].as_str() {
        summary.push_str(&format!(" ({scope})"));
    }
    summary.push('\n');
}

fn push_summary_field(summary: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        summary.push_str(&format!("  {label}: {value}\n"));
    }
}

fn array_len(value: &serde_json::Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or(0)
}

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(JsonOkFalse.into());
    }
    Ok(())
}

pub(crate) fn is_structured_json_failure(error: &anyhow::Error) -> bool {
    error.is::<JsonOkFalse>() || error.is::<VaultChildExitStatus>()
}

pub(crate) fn structured_error_exit_code(error: &anyhow::Error) -> Option<i32> {
    error
        .downcast_ref::<VaultChildExitStatus>()
        .map(|error| error.0)
}

fn parse_cli() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => exit_with_cli_error(error),
    }
}

fn exit_with_cli_error(error: clap::Error) -> ! {
    if should_add_template_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = moved_check_command_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = missing_init_path_hint(&error) {
        let message = error.to_string();
        // If stderr is closed, there is nowhere useful to report the parse hint.
        let _ = writeln!(std::io::stderr(), "{message}\n{hint}");
        process::exit(error.exit_code());
    }

    error.exit();
}

fn missing_init_path_hint(error: &clap::Error) -> Option<&'static str> {
    if error.kind() != ErrorKind::MissingRequiredArgument {
        return None;
    }

    if !error.context().any(|(kind, value)| {
        kind == ContextKind::Usage && context_contains(value, "jig init <PATH>")
    }) {
        return None;
    }

    Some(
        "\
`jig init` creates a new Jig-managed repository.
Use `jig adopt .` for an existing repository.

Use one of:
  jig init /path/to/new-repo --repo-name new-repo --sqlx-enabled false
  jig init /path/to/new-repo --preset rust-react
  jig init /path/to/new-repo --preset rust-react --db postgres --frontends web,landing,admin
  jig adopt .              # preview Jig adoption for this existing repo
  jig adopt . --write      # apply Jig adoption to this existing repo
  jig presets              # list available project scaffolds",
    )
}

pub(super) fn moved_check_command_hint(error: &clap::Error) -> Option<String> {
    if error.kind() != ErrorKind::InvalidSubcommand {
        return None;
    }

    let message = error.to_string();
    let moved = [
        ("fmt-check", "jig check fmt"),
        ("clippy", "jig check clippy"),
        ("test", "jig check test"),
        ("test-locked", "jig check test-locked"),
        ("sqlx-check", "jig check sqlx"),
        ("schema-check", "jig check schema"),
        ("contract-check", "jig check contract"),
        ("check-agent-guides", "jig check agent-guides"),
        ("check-rust-file-loc", "jig check rust-file-loc"),
        ("check-no-mod-rs", "jig check no-mod-rs"),
        (
            "check-migration-immutability",
            "jig check migration-immutability",
        ),
        (
            "check-sqlx-unchecked-non-test",
            "jig check sqlx-unchecked-non-test",
        ),
    ];

    // Like the nested agent-map case below, this depends on Clap 4.6.1 formatted
    // usage text and is only a best-effort migration hint. Global options such as
    // --json make the top-level usage line include [OPTIONS]; recheck this matcher
    // on Clap upgrades or when adding more global flags.
    if message.contains("Usage: jig [OPTIONS] <COMMAND>") {
        if let Some((_, replacement)) = moved
            .iter()
            .find(|(legacy, _)| message.contains(&format!("'{legacy}'")))
        {
            return Some(moved_check_hint_for(replacement));
        }
    }

    // Clap 4.6.1 reports nested invalid subcommands through formatted usage text;
    // this hint is best-effort and may disappear if that formatting changes.
    if message.contains("unrecognized subcommand 'check'")
        && message.contains("Usage: jig agent-map [OPTIONS] <COMMAND>")
    {
        return Some(moved_check_hint_for("jig check agent-map"));
    }

    None
}

fn moved_check_hint_for(replacement: &str) -> String {
    format!("This check command moved. Use:\n  {replacement}")
}

pub(super) fn should_add_template_hint(error: &clap::Error) -> bool {
    if !matches!(
        error.kind(),
        ErrorKind::InvalidValue | ErrorKind::TooFewValues
    ) {
        return false;
    }
    error
        .context()
        .any(|(kind, value)| kind == ContextKind::InvalidArg && context_mentions_template(value))
}

fn context_contains(value: &ContextValue, needle: &str) -> bool {
    match value {
        ContextValue::String(value) => value.contains(needle),
        ContextValue::Strings(values) => values.iter().any(|value| value.contains(needle)),
        ContextValue::StyledStr(value) => value.to_string().contains(needle),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| value.to_string().contains(needle)),
        _ => false,
    }
}

fn context_mentions_template(value: &ContextValue) -> bool {
    match value {
        ContextValue::String(value) => is_template_arg(value),
        ContextValue::Strings(values) => values.iter().any(|value| is_template_arg(value)),
        ContextValue::StyledStr(value) => is_template_arg(&value.to_string()),
        ContextValue::StyledStrs(values) => values
            .iter()
            .any(|value| is_template_arg(&value.to_string())),
        _ => false,
    }
}

fn is_template_arg(value: &str) -> bool {
    value
        .split_whitespace()
        .next()
        .is_some_and(|arg| arg == "--template")
}

#[cfg(test)]
// Keep these tests as children of `run` so formatter helpers can stay private
// to the CLI runtime instead of becoming module-public test surface.
#[path = "run_tests.rs"]
mod tests;
