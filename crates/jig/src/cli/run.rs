use std::io::Write;
use std::process;

use anyhow::Result;
use clap::Parser;
use clap::error::{ContextKind, ContextValue, ErrorKind};

use super::*;

const WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT: usize = 5;

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
            let output = crate::dev_proxy::commands::dev_without_context(opts)?;
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
            let output = runtime::dispatch(&ctx, CommandKind::Dev(opts))?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(not(feature = "dev-proxy"))]
        CommandKind::Proxy(command) => {
            let output = crate::dev_proxy::commands::proxy_without_context(command)?;
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        #[cfg(feature = "dev-proxy")]
        CommandKind::Proxy(command)
            if crate::dev_proxy::commands::can_run_without_context(&command) =>
        {
            let output = if let Some(ctx) = RepoContext::load_optional()? {
                runtime::dispatch(&ctx, CommandKind::Proxy(command))?
            } else {
                crate::dev_proxy::commands::proxy_without_context(command)?
            };
            print_json(&output)?;
            require_json_ok(true, &output)
        }
        other => {
            let require_ok = command_reports_failure_with_ok(&other);
            let human_output = human_output_requested(&other);
            let ctx = RepoContext::load()?;
            let output = runtime::dispatch(&ctx, other)?;
            print_output(human_output, &output)?;
            require_json_ok(require_ok, &output)
        }
    }
}

pub(super) fn command_reports_failure_with_ok(command: &CommandKind) -> bool {
    // Proxy commands expose host-cleanup/status operations that can complete
    // with `ok: false` in their JSON payload. Multi-app `jig dev` also uses
    // `ok: false` when the first child exits unsuccessfully. Agent doctor is a
    // readiness report and returns `ok: false` when required local tooling is
    // missing or unregistered.
    matches!(
        command,
        CommandKind::Dev(_)
            | CommandKind::Proxy(_)
            | CommandKind::Agent(AgentCommand::Doctor(_))
            | CommandKind::AgentMap(AgentMapCommand::Check(_))
            | CommandKind::CheckAgentGuides
            | CommandKind::CheckRustFileLoc(_)
            | CommandKind::CheckNoModRs
            | CommandKind::CheckMigrationImmutability(_)
            | CommandKind::CheckSqlxUncheckedNonTest
    )
}

enum HumanOutput {
    AgentDoctorSummary,
    WorkStatusSummary,
}

fn human_output_requested(command: &CommandKind) -> Option<HumanOutput> {
    match command {
        CommandKind::Agent(AgentCommand::Doctor(opts)) if opts.summary => {
            Some(HumanOutput::AgentDoctorSummary)
        }
        CommandKind::Work(WorkCommand::Status(opts)) if opts.summary => {
            Some(HumanOutput::WorkStatusSummary)
        }
        _ => None,
    }
}

pub(super) fn require_json_ok(required: bool, output: &serde_json::Value) -> Result<()> {
    if required && output.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
        return Err(JsonOkFalse.into());
    }
    Ok(())
}

#[derive(Debug)]
struct JsonOkFalse;

impl std::fmt::Display for JsonOkFalse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Command reported ok=false")
    }
}

impl std::error::Error for JsonOkFalse {}

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

    error.exit();
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
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn agent_doctor_summary_calls_out_source_mismatch() {
        let summary = format_agent_doctor_summary(&json!({
            "ok": false,
            "codex": {
                "required": true,
                "available": true
            },
            "marketplaces": [{
                "id": "jig-skills",
                "source": "bpcakes/jig-skills",
                "configured_source": "https://github.com/example/jig-skills.git",
                "registered": false
            }],
            "next_steps": [
                "Run `scripts/jig agent bootstrap` to register marketplace jig-skills."
            ]
        }));

        assert!(summary.contains("Agent tooling: needs setup"));
        assert!(summary.contains("repo config expects bpcakes/jig-skills"));
        assert!(summary.contains("Codex has https://github.com/example/jig-skills.git"));
        assert!(summary.contains("Next steps:"));
    }

    #[test]
    fn agent_doctor_summary_handles_optional_codex_requirement() {
        let summary = format_agent_doctor_summary(&json!({
            "ok": true,
            "codex": {
                "required": false,
                "available": null
            },
            "marketplaces": [],
            "next_steps": []
        }));

        assert!(summary.contains("Agent tooling: ready"));
        assert!(summary.contains("Codex: not required (probe skipped)"));
        assert!(summary.contains("Marketplaces: none configured"));
        // Regression guard for the previously duplicated requirement/probe label.
        assert!(!summary.contains("not required (not required)"));
        // When Codex is not required, the summary should explain the skipped
        // probe instead of exposing the underlying null availability field.
        assert!(!summary.contains("unknown"));
    }

    #[test]
    fn agent_doctor_summary_handles_ready_marketplace() {
        let summary = format_agent_doctor_summary(&json!({
            "ok": true,
            "codex": {
                "required": true,
                "available": true
            },
            "marketplaces": [{
                "id": "jig-skills",
                "source": "bpcakes/jig-skills",
                "configured_source": "https://github.com/bpcakes/jig-skills.git",
                "registered": true
            }],
            "next_steps": []
        }));

        assert!(summary.contains("Agent tooling: ready"));
        assert!(summary.contains("Codex: required (available)"));
        assert!(summary.contains("jig-skills: registered"));
        assert!(summary.contains("Next steps: none"));
    }

    #[test]
    fn agent_doctor_summary_handles_unknown_required_codex_availability() {
        let summary = format_agent_doctor_summary(&json!({
            "ok": false,
            "codex": {
                "required": true,
                "available": null
            },
            "marketplaces": [],
            "next_steps": []
        }));

        assert!(summary.contains("Codex: required (unknown)"));
    }

    #[test]
    fn work_status_summary_stays_compact() {
        let summary = format_work_status_summary(&json!({
            "repo": {
                "name": "demo",
                "default_branch": "main"
            },
            "current_session_id": null,
            "counts": {
                "open_plans": 1,
                "receipts": 12,
                "failed_receipts": 2,
                "decisions": 3
            },
            "open_plans": [{
                "plan_id": "plan_1",
                "title": "Improve UX"
            }],
            "recent_receipts": [
                {
                    "id": "receipt_1",
                    "tool_name": "jig.test",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_2",
                    "tool_name": "jig.clippy",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_3",
                    "tool_name": "jig.fmt_check",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_4",
                    "tool_name": "jig.contract_check",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_5",
                    "tool_name": "jig.bootstrap",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_6",
                    "tool_name": "jig.extra",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                }
            ]
        }));

        assert!(summary.contains("Work status:"));
        assert!(summary.contains("Plans: 1 open"));
        assert!(summary.contains("Receipts: 12 total, 2 failed"));
        assert!(summary.contains("Decisions: 3"));
        assert!(summary.contains("Repo: demo (main)"));
        assert!(summary.contains("Current session: none"));
        assert!(summary.contains("plan_1: Improve UX"));
        assert!(summary.contains("jig.test"));
        assert!(summary.contains("and 1 more recent receipt"));
    }

    #[test]
    fn work_status_summary_omits_truncation_hint_at_receipt_limit() {
        let summary = format_work_status_summary(&json!({
            "repo": {
                "name": "demo",
                "default_branch": "main"
            },
            "current_session_id": null,
            "counts": {
                "open_plans": 0,
                "receipts": 5,
                "failed_receipts": 0,
                "decisions": 0
            },
            "open_plans": [],
            "recent_receipts": [
                {
                    "id": "receipt_1",
                    "tool_name": "jig.test",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_2",
                    "tool_name": "jig.clippy",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_3",
                    "tool_name": "jig.fmt_check",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_4",
                    "tool_name": "jig.contract_check",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                },
                {
                    "id": "receipt_5",
                    "tool_name": "jig.bootstrap",
                    "exit_status": 0,
                    "diff_summary": "no changes"
                }
            ]
        }));

        assert!(summary.contains("receipt_5"));
        assert!(!summary.contains("omit --summary"));
    }

    #[test]
    fn work_status_summary_handles_empty_state() {
        let summary = format_work_status_summary(&json!({
            "repo": {
                "name": "demo",
                "default_branch": "main"
            },
            "current_session_id": null,
            "counts": {
                "open_plans": 0,
                "receipts": 0,
                "failed_receipts": 0,
                "decisions": 0
            },
            "open_plans": [],
            "recent_receipts": []
        }));

        assert!(summary.contains("Plans: 0 open"));
        assert!(summary.contains("Receipts: 0 total, 0 failed"));
        assert!(summary.contains("Decisions: 0"));
        assert!(summary.contains("Current session: none"));
        assert!(summary.contains("Open plans: none"));
        assert!(summary.contains("Recent receipts: none"));
    }
}
