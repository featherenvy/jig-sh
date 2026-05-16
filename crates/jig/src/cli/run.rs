use std::io::Write;
use std::process;

use anyhow::Result;
use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};

use super::*;

const WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT: usize = 5;

enum HumanOutput {
    AgentDoctorSummary,
    WorkStartPlanId,
    WorkReceiptsSummary,
    WorkStatusSummary,
}

#[derive(Debug)]
struct JsonOkFalse;

impl std::fmt::Display for JsonOkFalse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command reported ok=false")
    }
}

impl std::error::Error for JsonOkFalse {}

pub(crate) fn run() -> Result<()> {
    let cli = parse_cli();
    match cli.command {
        CommandKind::Init(opts) => print_json(&bootstrap::run_init(opts)?),
        CommandKind::Adopt(opts) => print_json(&bootstrap::run_adopt(opts)?),
        CommandKind::Update(opts) => print_json(&bootstrap::run_update(opts)?),
        CommandKind::Mcp => {
            let ctx = RepoContext::load()?;
            mcp::serve(&ctx)
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
        CommandKind::RunTarget(opts) => dispatch_runtime_command(
            crate::command::RuntimeCommand::RunTarget(opts.into()),
            false,
            None,
        ),
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
        CommandKind::Dev(_) | CommandKind::Proxy(_) => true,
        CommandKind::Agent(command) => agent_command_reports_failure_with_ok(command),
        CommandKind::Check(command) => check_command_reports_failure_with_ok(command),
        _ => false,
    }
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

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(JsonOkFalse.into());
    }
    Ok(())
}

pub(crate) fn is_structured_json_failure(error: &anyhow::Error) -> bool {
    error.is::<JsonOkFalse>()
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
        let _ = writeln!(std::io::stderr(), "{message}\n{TEMPLATE_ERROR_HINT}");
        process::exit(error.exit_code());
    }

    if let Some(hint) = moved_check_command_hint(&error) {
        let message = error.to_string();
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
    // usage text and is only a best-effort migration hint. Recheck on Clap upgrades.
    if message.contains("Usage: jig <COMMAND>") {
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
        && message.contains("Usage: jig agent-map <COMMAND>")
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

fn print_json(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    handle.write_all(b"\n")?;
    Ok(())
}

fn print_output(human_output: Option<HumanOutput>, value: &serde_json::Value) -> Result<()> {
    match human_output {
        Some(HumanOutput::AgentDoctorSummary) => print_text(&format_agent_doctor_summary(value)),
        Some(HumanOutput::WorkStartPlanId) => print_text(&format_work_start_plan_id(value)?),
        Some(HumanOutput::WorkReceiptsSummary) => print_text(&format_work_receipts_summary(value)),
        Some(HumanOutput::WorkStatusSummary) => print_text(&format_work_status_summary(value)),
        None => print_json(value),
    }
}

fn print_text(text: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(text.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}

fn format_work_start_plan_id(value: &serde_json::Value) -> Result<String> {
    let plan = value
        .get("plan")
        .ok_or_else(|| anyhow::anyhow!("work start output did not include plan"))?;
    if !plan.is_object() {
        anyhow::bail!("work start output plan was not an object");
    }

    plan.get("plan_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("work start output did not include plan.plan_id"))
}

fn format_agent_doctor_summary(value: &serde_json::Value) -> String {
    let ready = value_bool(value, "ok").unwrap_or(false);
    let codex = &value["codex"];
    let codex_required = value_bool(codex, "required").unwrap_or(false);
    let codex_line = if codex_required {
        let codex_available = codex
            .get("available")
            .and_then(serde_json::Value::as_bool)
            .map(|available| {
                if available {
                    "available"
                } else {
                    "unavailable"
                }
            })
            .unwrap_or("unknown");
        format!("Codex: required ({codex_available})")
    } else {
        "Codex: not required (probe skipped)".into()
    };
    let mut lines = vec![
        format!(
            "Agent tooling: {}",
            if ready { "ready" } else { "needs setup" }
        ),
        codex_line,
    ];

    let marketplaces = value["marketplaces"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if marketplaces.is_empty() {
        lines.push("Marketplaces: none configured".into());
    } else {
        lines.push("Marketplaces:".into());
        for marketplace in marketplaces {
            let id = value_str(marketplace, "id").unwrap_or("<unknown>");
            let source = value_str(marketplace, "source").unwrap_or("<unknown>");
            let registered = value_bool(marketplace, "registered").unwrap_or(false);
            let configured = value_str(marketplace, "configured_source");
            let detail = match (registered, configured) {
                (true, _) => format!("registered ({source})"),
                (false, Some(configured)) => {
                    format!("not registered; repo config expects {source}, Codex has {configured}")
                }
                (false, None) => format!("missing registration for {source}"),
            };
            lines.push(format!("  - {id}: {detail}"));
        }
    }

    let next_steps = value["next_steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if next_steps.is_empty() {
        lines.push("Next steps: none".into());
    } else {
        lines.push("Next steps:".into());
        for step in next_steps {
            if let Some(step) = step.as_str() {
                lines.push(format!("  - {step}"));
            }
        }
    }

    lines.join("\n")
}

fn format_work_status_summary(value: &serde_json::Value) -> String {
    let counts = &value["counts"];
    let repo = &value["repo"];
    let repo_name = value_str(repo, "name").unwrap_or("<unknown>");
    let default_branch = value_str(repo, "default_branch").unwrap_or("<unknown>");
    let open_plan_count = value_u64(counts, "open_plans").unwrap_or(0);
    let receipt_count = value_u64(counts, "receipts").unwrap_or(0);
    let failed_receipt_count = value_u64(counts, "failed_receipts").unwrap_or(0);
    let decision_count = value_u64(counts, "decisions").unwrap_or(0);

    let mut lines = vec![
        "Work status:".into(),
        format!("  Plans: {open_plan_count} open"),
        format!("  Receipts: {receipt_count} total, {failed_receipt_count} failed"),
        format!("  Decisions: {decision_count}"),
        format!("Repo: {repo_name} ({default_branch})"),
        format!(
            "Current session: {}",
            value_str(value, "current_session_id").unwrap_or("none")
        ),
    ];

    let open_plans = value["open_plans"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if open_plans.is_empty() {
        lines.push("Open plans: none".into());
    } else {
        lines.push("Open plans:".into());
        for plan in open_plans {
            let plan_id = value_str(plan, "plan_id").unwrap_or("<unknown>");
            let title = value_str(plan, "title").unwrap_or("<untitled>");
            lines.push(format!("  - {plan_id}: {title}"));
        }
    }

    let recent_receipts = value["recent_receipts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if recent_receipts.is_empty() {
        lines.push("Recent receipts: none".into());
    } else {
        lines.push("Recent receipts:".into());
        for receipt in recent_receipts
            .iter()
            .take(WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT)
        {
            let id = value_str(receipt, "id").unwrap_or("<unknown>");
            let tool = value_str(receipt, "tool_name").unwrap_or("<unknown>");
            let exit_status = value_i64(receipt, "exit_status")
                .map(|status| status.to_string())
                .unwrap_or_else(|| "?".into());
            let diff = value_str(receipt, "diff_summary").unwrap_or("unknown diff");
            lines.push(format!("  - {tool} ({id}): exit {exit_status}, {diff}"));
        }
        if recent_receipts.len() > WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT {
            let hidden = recent_receipts.len() - WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT;
            let noun = if hidden == 1 { "receipt" } else { "receipts" };
            lines.push(format!(
                "  (and {hidden} more recent {noun}; omit --summary for full JSON)"
            ));
        }
    }

    lines.join("\n")
}

fn format_work_receipts_summary(value: &serde_json::Value) -> String {
    let receipts = value["receipts"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut lines = vec![
        "Work receipts:".into(),
        format!("  Showing: {}", receipts.len()),
    ];

    if receipts.is_empty() {
        lines.push("  No receipts matched the selected filters.".into());
        return lines.join("\n");
    }

    for receipt in receipts {
        let id = value_str(receipt, "id").unwrap_or("<unknown>");
        let tool = value_str(receipt, "tool_name").unwrap_or("<unknown>");
        let exit_status = value_i64(receipt, "exit_status")
            .map(|status| status.to_string())
            .unwrap_or_else(|| "?".into());
        let diff = value_str(receipt, "diff_summary").unwrap_or("unknown diff");
        lines.push(format!("  - {tool} ({id}): exit {exit_status}, {diff}"));

        let plan = value_str(receipt, "plan_id").unwrap_or("none");
        let session = value_str(receipt, "session_id").unwrap_or("none");
        lines.push(format!("    plan: {plan}; session: {session}"));

        if let Some(preview) = receipt_preview(receipt) {
            lines.push(format!("    output: {preview}"));
        }
    }

    lines.join("\n")
}

fn receipt_preview(receipt: &serde_json::Value) -> Option<String> {
    value_str(receipt, "stderr_preview")
        .filter(|preview| !preview.trim().is_empty())
        .or_else(|| {
            value_str(receipt, "stdout_preview").filter(|preview| !preview.trim().is_empty())
        })
        .map(|preview| concise_preview(preview, 180))
}

fn concise_preview(preview: &str, max_chars: usize) -> String {
    let one_line = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return one_line;
    }

    // Receipt previews are diagnostic text; truncate on scalar boundaries so
    // UTF-8 stays valid, accepting that grapheme clusters may split.
    let mut truncated = one_line
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn value_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn value_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(serde_json::Value::as_bool)
}

fn value_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

fn value_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
// Keep these tests as children of `run` so formatter helpers can stay private
// to the CLI runtime instead of becoming module-public test surface.
#[path = "run_tests.rs"]
mod tests;
