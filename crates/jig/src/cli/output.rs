use std::io::Write;

use anyhow::{Result, anyhow};

use crate::{doctor, info};

const WORK_STATUS_RECENT_RECEIPT_SUMMARY_LIMIT: usize = 5;

pub(super) enum HumanOutput {
    DoctorSummary,
    InfoSummary,
    VaultRunSummary,
    AgentDoctorSummary,
    WorkCheckSummary,
    WorkGatesSummary,
    WorkEvidenceSummary,
    WorkStartPlanId,
    WorkReceiptsSummary,
    WorkStatusSummary,
}

pub(super) fn print_output(
    human_output: Option<HumanOutput>,
    value: &serde_json::Value,
) -> Result<()> {
    match human_output {
        Some(HumanOutput::DoctorSummary) => print_text(&doctor::format_summary(value)),
        Some(HumanOutput::InfoSummary) => print_text(&info::format_summary(value)),
        Some(HumanOutput::VaultRunSummary) => print_text(&format_vault_run_summary(value)),
        Some(HumanOutput::AgentDoctorSummary) => print_text(&format_agent_doctor_summary(value)),
        Some(HumanOutput::WorkCheckSummary) => print_text(&format_work_check_summary(value)),
        Some(HumanOutput::WorkGatesSummary) => print_text(&format_work_gates_summary(value)),
        Some(HumanOutput::WorkEvidenceSummary) => print_text(&format_work_evidence_summary(value)),
        Some(HumanOutput::WorkStartPlanId) => print_text(&format_work_start_plan_id(value)?),
        Some(HumanOutput::WorkReceiptsSummary) => print_text(&format_work_receipts_summary(value)),
        Some(HumanOutput::WorkStatusSummary) => print_text(&format_work_status_summary(value)),
        None => print_json(value),
    }
}

pub(super) fn print_json(value: &serde_json::Value) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    serde_json::to_writer_pretty(&mut handle, value)?;
    handle.write_all(b"\n")?;
    // `jig vault run` may return a structured non-zero child status after
    // printing, so flush before unwinding through main.
    handle.flush()?;
    Ok(())
}

fn print_text(text: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(text.as_bytes())?;
    handle.write_all(b"\n")?;
    Ok(())
}

pub(super) fn format_vault_run_summary(value: &serde_json::Value) -> String {
    let result = &value["result"];
    let exit_status = value_i64(result, "exit_status")
        .map(|status| status.to_string())
        .unwrap_or_else(|| "?".into());
    let mut lines = vec![format!("Vault run: exit {exit_status}")];
    if let Some(signal) = value_i64(result, "exit_signal") {
        lines.push(format!("  Signal: {signal}"));
    }
    let mut truncated = false;
    if let Some(stdout) = value_str(result, "stdout").filter(|text| !text.is_empty()) {
        let (preview, was_truncated) = concise_preview_with_truncation(stdout, 240);
        truncated |= was_truncated;
        lines.push(format!("  stdout: {preview}"));
    }
    if let Some(stderr) = value_str(result, "stderr").filter(|text| !text.is_empty()) {
        let (preview, was_truncated) = concise_preview_with_truncation(stderr, 240);
        truncated |= was_truncated;
        lines.push(format!("  stderr: {preview}"));
    }
    if truncated {
        lines.push("  Output truncated; rerun without --summary for full JSON.".into());
    }
    lines.join("\n")
}

pub(super) fn format_work_start_plan_id(value: &serde_json::Value) -> Result<String> {
    let plan = value
        .get("plan")
        .ok_or_else(|| anyhow!("work start output did not include plan"))?;
    if !plan.is_object() {
        anyhow::bail!("work start output plan was not an object");
    }

    plan.get("plan_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("work start output did not include plan.plan_id"))
}

pub(super) fn format_agent_doctor_summary(value: &serde_json::Value) -> String {
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

pub(super) fn format_work_status_summary(value: &serde_json::Value) -> String {
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

pub(super) fn format_work_check_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let checks = value["checks"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let status = work_check_summary_status(checks);
    let skipped_checks = checks
        .iter()
        .filter(|check| {
            check["result"]["exit_status"].as_i64() == Some(0)
                && work_check_summary_harness_skip_output(check).is_some()
        })
        .count();
    let status_label = if matches!(status, WorkCheckSummaryStatus::Passed) {
        match (skipped_checks, checks.len()) {
            (0, _) => status.label(),
            (skipped, total) if skipped == total => "passed (all skipped)",
            _ => "passed (some skipped)",
        }
    } else {
        status.label()
    };
    let mut lines = vec![
        format!("Work check: {status_label}"),
        format!("  Plan: {plan_id}"),
        format!(
            "  Batch receipt: {}",
            value_str(value, "receipt_id").unwrap_or("none")
        ),
        format!("  Checks: {}", checks.len()),
    ];

    for check in checks {
        let tool = value_str(check, "tool").unwrap_or("<unknown>");
        let exit_status = check["result"]["exit_status"].as_i64();
        let exit_status_label = exit_status
            .map(|status| status.to_string())
            .unwrap_or_else(|| "?".into());
        let receipt = value_str(check, "receipt_id").unwrap_or("none");
        let output_note = work_check_summary_output_note(check, exit_status)
            .map(|note| format!(", output: {note}"))
            .unwrap_or_default();
        lines.push(format!(
            "  - {tool}: exit {exit_status_label}, receipt {receipt}{output_note}"
        ));
    }

    if skipped_checks > 0 && skipped_checks == checks.len() {
        lines.push(
            "Note: all configured Cargo checks skipped because no root Cargo.toml exists; set explicit commands if this repo has Rust code outside a root workspace.".into(),
        );
    }

    match status {
        WorkCheckSummaryStatus::Passed => lines.push(format!(
            "Next step: scripts/jig work gates --plan-id {plan_id} --summary"
        )),
        WorkCheckSummaryStatus::Failed => lines.push(format!(
            "Next step: inspect failing receipts, fix issues, then rerun scripts/jig work check --plan-id {plan_id} --summary"
        )),
        WorkCheckSummaryStatus::Unknown => lines.push(format!(
            "Next step: inspect receipts with unknown exit status, then rerun scripts/jig work check --plan-id {plan_id} --summary"
        )),
        WorkCheckSummaryStatus::NoChecksConfigured => lines.push(format!(
            "Next step: configure work checks or rerun scripts/jig work check --plan-id {plan_id} --tool <tool> --summary"
        )),
    }
    lines.join("\n")
}

fn work_check_summary_output_note(
    check: &serde_json::Value,
    exit_status: Option<i64>,
) -> Option<String> {
    if exit_status == Some(0)
        && let Some(output) = work_check_summary_harness_skip_output(check)
    {
        return Some(concise_preview(output, 120));
    }

    let result = &check["result"];
    let stdout = value_str(result, "stdout").filter(|output| !output.trim().is_empty());
    let stderr = value_str(result, "stderr").filter(|output| !output.trim().is_empty());
    match exit_status {
        Some(0) => None,
        Some(_) | None => stderr.or(stdout).map(|output| concise_preview(output, 120)),
    }
}

fn work_check_summary_harness_skip_output(check: &serde_json::Value) -> Option<&str> {
    let result = &check["result"];
    value_str(result, "stdout")
        .filter(|output| !output.trim().is_empty())
        .filter(|output| work_check_summary_has_harness_skip(output))
}

fn work_check_summary_has_harness_skip(output: &str) -> bool {
    output.lines().any(|line| {
        line.trim_start()
            .starts_with(crate::CARGO_SKIP_OUTPUT_PREFIX)
    })
}

#[derive(Clone, Copy)]
enum WorkCheckSummaryStatus {
    Passed,
    Failed,
    Unknown,
    NoChecksConfigured,
}

impl WorkCheckSummaryStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
            Self::NoChecksConfigured => "no checks configured",
        }
    }
}

fn work_check_summary_status(checks: &[serde_json::Value]) -> WorkCheckSummaryStatus {
    if checks.is_empty() {
        return WorkCheckSummaryStatus::NoChecksConfigured;
    }

    let mut saw_unknown = false;
    for check in checks {
        match check["result"]["exit_status"].as_i64() {
            Some(0) => {}
            Some(_) => return WorkCheckSummaryStatus::Failed,
            None => saw_unknown = true,
        }
    }

    if saw_unknown {
        WorkCheckSummaryStatus::Unknown
    } else {
        WorkCheckSummaryStatus::Passed
    }
}

pub(super) fn format_work_gates_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let plan_state = value_str(value, "plan_state").unwrap_or("open");
    let overall = value_str(value, "overall").unwrap_or("unknown");
    let gates = value["gates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut lines = vec![
        format!("Work gates: {overall}"),
        format_work_plan_line(plan_id, plan_state),
        format!("  Gates: {}", gates.len()),
    ];

    for gate in gates {
        let id = value_str(gate, "id").unwrap_or("<unknown>");
        let status = value_str(gate, "status").unwrap_or("unknown");
        let required = value_bool(gate, "required").unwrap_or(true);
        let required_label = if required { "required" } else { "optional" };
        let tool = value_str(gate, "tool")
            .map(|tool| format!(" ({tool})"))
            .unwrap_or_default();
        let freshness = value_str(gate, "freshness")
            .map(|freshness| format!(", freshness {freshness}"))
            .unwrap_or_default();
        let mut line = format!("  - {id}: {status}{freshness}, {required_label}{tool}");
        if !matches!(status, "passed" | "missing") {
            if let Some(reason) = value_str(gate, "freshness_reason") {
                line.push_str(&format!("; {reason}"));
            }
        }
        lines.push(line);
        if status != "missing" {
            if let Some(diff) = value_str(gate, "diff_summary").filter(|diff| !diff.is_empty()) {
                lines.push(format!("    receipt diff: {diff}"));
            }
        }
        let changed_paths = value_string_list(gate, "changed_paths");
        if status != "missing" && !changed_paths.is_empty() {
            lines.push(format!(
                "    changed paths covered: {}",
                changed_paths.join(", ")
            ));
        }
    }

    if overall == "passed" && plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id} --resolution <summary> --outcome success"
        ));
    } else if overall == "passed" {
        lines.push("Next step: none; plan is closed".into());
    } else {
        match gate_blocker_summary(value) {
            Some(blockers) => lines.push(format!("Blocked: {blockers}")),
            None => lines.push(format!(
                "Status: {overall}; no categorized blockers reported"
            )),
        }
        if plan_state == "open" {
            lines.push(format!(
                "Next step: scripts/jig work check --plan-id {plan_id} --summary"
            ));
        } else {
            lines.push("Next step: start a new work plan for follow-up changes".into());
        }
    }

    lines.join("\n")
}

pub(super) fn format_work_evidence_summary(value: &serde_json::Value) -> String {
    let plan_id = value_str(value, "plan_id").unwrap_or("<unknown>");
    let plan_state = value_str(value, "plan_state").unwrap_or("open");
    let overall = value_str(value, "overall").unwrap_or("unknown");
    let latest = value["latest_passing_gates"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let gates = value["gates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut lines = vec![
        format!("Work evidence: {overall}"),
        format_work_plan_line(plan_id, plan_state),
    ];

    if latest.is_empty() {
        lines.push("Latest gate evidence per tool: none".into());
    } else {
        lines.push("Latest gate evidence per tool:".into());
        for gate in latest {
            let tool = value_str(gate, "tool").unwrap_or("<unknown>");
            let gate_id = value_str(gate, "gate_id").unwrap_or("<unknown>");
            let receipt = value_str(gate, "freshness_receipt_id")
                .or_else(|| value_str(gate, "receipt_id"))
                .unwrap_or("none");
            let freshness = value_str(gate, "freshness").unwrap_or("unknown");
            let matches = value_bool(gate, "matches_current_worktree").unwrap_or(false);
            let matches_label = if matches { "yes" } else { "no" };
            lines.push(format!(
                "  - {tool}: {gate_id}, receipt {receipt}, matches current worktree {matches_label} ({freshness})"
            ));
            if let Some(reason) = value_str(gate, "freshness_reason") {
                lines.push(format!("    reason: {reason}"));
            }
            if let Some(diff) = value_str(gate, "diff_summary").filter(|diff| !diff.is_empty()) {
                lines.push(format!("    receipt diff: {diff}"));
            }
            let changed_paths = value_string_list(gate, "changed_paths");
            if !changed_paths.is_empty() {
                lines.push(format!(
                    "    changed paths covered: {}",
                    changed_paths.join(", ")
                ));
            }
        }
    }

    let unresolved = gates
        .iter()
        .filter(|gate| value_str(gate, "status") != Some("passed"))
        .collect::<Vec<_>>();
    if unresolved.is_empty() {
        lines.push("Unresolved gates: none".into());
    } else {
        lines.push("Unresolved gates:".into());
        for gate in unresolved {
            let id = value_str(gate, "id").unwrap_or("<unknown>");
            let status = value_str(gate, "status").unwrap_or("unknown");
            let reason =
                value_str(gate, "freshness_reason").unwrap_or("no receipt evidence for this gate");
            lines.push(format!("  - {id}: {status}; {reason}"));
        }
    }

    if overall == "passed" && plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work finish --plan-id {plan_id} --resolution <summary> --outcome success"
        ));
    } else if overall == "passed" {
        lines.push("Next step: none; plan is closed".into());
    } else if plan_state == "open" {
        lines.push(format!(
            "Next step: scripts/jig work check --plan-id {plan_id} --summary"
        ));
    } else {
        lines.push("Next step: start a new work plan for follow-up changes".into());
    }

    lines.join("\n")
}

fn format_work_plan_line(plan_id: &str, plan_state: &str) -> String {
    if plan_state == "closed" {
        format!("  Plan: {plan_id} (closed)")
    } else {
        format!("  Plan: {plan_id}")
    }
}

fn gate_blocker_summary(value: &serde_json::Value) -> Option<String> {
    let categories = [
        ("missing", "missing_required"),
        ("failed", "failed_required"),
        ("stale", "stale_required"),
        ("unknown", "unknown_required"),
        ("unsupported", "unsupported_required"),
    ];
    let blockers = categories
        .into_iter()
        .filter_map(|(label, key)| {
            let items = value_string_list(value, key);
            (!items.is_empty()).then(|| format!("{label} ({})", items.join(", ")))
        })
        .collect::<Vec<_>>();

    (!blockers.is_empty()).then(|| blockers.join("; "))
}

pub(super) fn format_work_receipts_summary(value: &serde_json::Value) -> String {
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
    concise_preview_with_truncation(preview, max_chars).0
}

fn concise_preview_with_truncation(preview: &str, max_chars: usize) -> (String, bool) {
    let trimmed = preview.trim();
    if trimmed.chars().count() <= max_chars {
        return (trimmed.to_string(), false);
    }

    let one_line = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return (one_line, false);
    }

    // Receipt previews are diagnostic text; truncate on scalar boundaries so
    // UTF-8 stays valid, accepting that grapheme clusters may split.
    let mut truncated = one_line
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    (truncated, true)
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

fn value_string_list(value: &serde_json::Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}
