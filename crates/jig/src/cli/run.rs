use std::io::{self, Write};
use std::process;

use anyhow::Result;
use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};

use super::output::{HumanOutput, print_json, print_output};
#[cfg(test)]
use super::output::{
    format_agent_doctor_summary, format_vault_run_summary, format_work_check_summary,
    format_work_evidence_summary, format_work_gates_summary, format_work_receipts_summary,
    format_work_start_plan_id, format_work_status_summary,
};
use super::*;

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
        CommandKind::Init(opts) => print_json(&bootstrap::run_init(opts)?),
        CommandKind::Adopt(opts) => {
            let output = bootstrap::run_adopt(opts)?;
            if json_output {
                print_json(&output)?;
            } else {
                print_adopt_human_summary(&output)?;
            }
            Ok(())
        }
        CommandKind::Update(opts) => print_json(&bootstrap::run_update(opts)?),
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
            let is_run = matches!(command, VaultCommand::Run(_));
            if vault_command_requires_passphrase(&command) {
                // Invariant: capture and clear the process environment copy
                // before vault runtime code can start background threads.
                if matches!(command, VaultCommand::Init(_)) {
                    runtime::capture_new_vault_passphrase()?;
                } else {
                    runtime::capture_vault_passphrase()?;
                }
            }
            let output = runtime::dispatch_vault(command.into())?;
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
        CommandKind::Vault(command) => vault_command_reports_failure_with_ok(command),
        CommandKind::Agent(command) => agent_command_reports_failure_with_ok(command),
        CommandKind::Check(command) => check_command_reports_failure_with_ok(command),
        _ => false,
    }
}

#[cfg(test)]
fn vault_command_reports_failure_with_ok(command: &VaultCommand) -> bool {
    matches!(command, VaultCommand::Run(_))
}

fn vault_command_requires_passphrase(command: &VaultCommand) -> bool {
    !matches!(command, VaultCommand::Status(_))
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

fn print_adopt_human_summary(output: &serde_json::Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(format_adopt_human_summary(output).as_bytes())?;
    Ok(())
}

pub(super) fn format_adopt_human_summary(output: &serde_json::Value) -> String {
    let mut summary = String::new();
    summary.push_str("adopt summary\n");
    push_summary_field(&mut summary, "mode", output["render_mode"].as_str());
    push_summary_field(&mut summary, "target", output["destination"].as_str());

    let report = &output["adoption_report"];
    let created = array_len(&report["files_created"]);
    let modified = array_len(&report["files_modified"]);
    let removed = array_len(&report["files_removed"]);
    summary.push_str(&format!(
        "  managed files: {created} created, {modified} modified, {removed} removed\n"
    ));

    if let Some(review) = output["adoption_review"].as_array()
        && !review.is_empty()
    {
        summary.push_str("  review:\n");
        for item in review.iter().filter_map(serde_json::Value::as_str) {
            summary.push_str(&format!("    - {item}\n"));
        }
    }

    if let Some(conflicts) = report["conflicts"].as_array()
        && !conflicts.is_empty()
    {
        summary.push_str(&format!("  conflicts: {}\n", conflicts.len()));
        for conflict in conflicts.iter().take(10) {
            if let Some(path) = conflict["path"].as_str() {
                if let Some(detail) = conflict["detail"].as_str() {
                    summary.push_str(&format!("    - {path}: {detail}\n"));
                } else {
                    summary.push_str(&format!("    - {path}\n"));
                }
            }
        }
        if conflicts.len() > 10 {
            summary.push_str(&format!("    - and {} more\n", conflicts.len() - 10));
        }
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

    error.exit();
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
