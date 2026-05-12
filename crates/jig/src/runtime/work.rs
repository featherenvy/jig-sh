use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::{WorkCheckOpts, WorkCommand, WorkFinishOpts, WorkGatesOpts};
use crate::context::{RepoContext, WorkGateConfig};
use crate::state::{
    ReceiptInput, current_session, current_worktree_fingerprint, decisions_add,
    latest_plan_tool_receipt, latest_plan_work_check_receipt_for_tool, now_ms, plans_append,
    plans_close, plans_open, receipts_list, record_receipt, session_end, session_start,
    state_summary,
};
use crate::tool_defs::{
    self, JsonObject, args, required_string_arg, string_arg, string_list_arg, tool,
};

use super::{execute_manifest_make_tool_without_worktree_fingerprint, requests};

pub(super) fn dispatch(ctx: &RepoContext, command: WorkCommand) -> Result<Value> {
    match command {
        WorkCommand::Start(opts) => start(ctx, opts.into()),
        WorkCommand::Append(opts) => plans_append(ctx, opts.into()),
        WorkCommand::Check(opts) => check(ctx, opts),
        WorkCommand::Gates(opts) => gates(ctx, opts),
        WorkCommand::Decide(opts) => decisions_add(ctx, opts.into()),
        WorkCommand::Receipts(opts) => receipts_list(ctx, opts.into()),
        WorkCommand::Status => state_summary(ctx),
        WorkCommand::Finish(opts) => finish(ctx, opts),
    }
}

pub(super) fn start(ctx: &RepoContext, plan: crate::state::PlanOpenRequest) -> Result<Value> {
    let session = session_start(ctx)?;
    let plan = plans_open(ctx, plan)?;

    Ok(json!({
        "ok": true,
        "session": session,
        "plan": plan,
    }))
}

pub(super) fn check(ctx: &RepoContext, opts: WorkCheckOpts) -> Result<Value> {
    check_tools(ctx, &opts.plan_id, selected_tools(ctx, &opts.tools)?)
}

pub(super) fn gates(ctx: &RepoContext, opts: WorkGatesOpts) -> Result<Value> {
    gate_status(ctx, &opts.plan_id)
}

pub(super) fn finish(ctx: &RepoContext, opts: WorkFinishOpts) -> Result<Value> {
    ensure_required_gates_passed(ctx, &opts.plan_id)?;

    let plan = plans_close(ctx, (&opts).into())?;
    let session = match current_session(ctx)? {
        Some(_) => Some(session_end(
            ctx,
            requests::session_end_request_for_finish(opts.outcome.or(opts.resolution)),
        )?),
        None => None,
    };

    Ok(json!({
        "ok": true,
        "plan": plan,
        "session": session,
    }))
}

pub(super) fn start_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    start(ctx, requests::plan_open_request_from_args(args_obj)?)
}

pub(super) fn append_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    plans_append(ctx, requests::plan_append_request_from_args(args_obj)?)
}

pub(super) fn check_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    let plan_id = required_string_arg(args_obj, args::PLAN_ID)?;
    check_tools(
        ctx,
        &plan_id,
        selected_tools(ctx, &string_list_arg(args_obj, args::TOOLS))?,
    )
}

pub(super) fn gates_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    gates(
        ctx,
        WorkGatesOpts {
            plan_id: required_string_arg(args_obj, args::PLAN_ID)?,
        },
    )
}

pub(super) fn decide_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    decisions_add(ctx, requests::decision_add_request_from_args(args_obj)?)
}

pub(super) fn receipts_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    receipts_list(
        ctx,
        requests::receipt_list_filter_from_args(args_obj, crate::cli::DEFAULT_RECEIPTS_LIMIT),
    )
}

pub(super) fn finish_from_args(ctx: &RepoContext, args_obj: &JsonObject) -> Result<Value> {
    let plan_id = required_string_arg(args_obj, args::PLAN_ID)?;
    let resolution = string_arg(args_obj, args::RESOLUTION);
    let outcome = string_arg(args_obj, args::OUTCOME);
    finish(
        ctx,
        WorkFinishOpts {
            plan_id,
            resolution,
            outcome,
        },
    )
}

fn selected_tools(ctx: &RepoContext, explicit_tools: &[String]) -> Result<Vec<String>> {
    let tools = if explicit_tools.is_empty() {
        ctx.work_check_tools()
    } else {
        explicit_tools.to_vec()
    };

    if tools.is_empty() {
        bail!("No work check gates configured. Add work.gates to .jig.yml or pass --tool.");
    }

    Ok(tools)
}

fn check_tools(ctx: &RepoContext, plan_id: &str, tools: Vec<String>) -> Result<Value> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    let mut results = Vec::with_capacity(tools.len());
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;

        results.push(execute_manifest_make_tool_without_worktree_fingerprint(
            ctx,
            name,
            json!({}),
            Some(plan_id.to_string()),
        )?);
    }
    let after_fingerprint = current_worktree_fingerprint(ctx);
    let worktree_fingerprint_override =
        work_check_fingerprint_evidence(&before_fingerprint, &after_fingerprint);
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": plan_id,
                "tools": tools,
            }),
            invoked_make_target: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(worktree_fingerprint_override),
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "checks": results,
        "receipt_id": receipt_id,
    }))
}

fn work_check_fingerprint_evidence(
    before: &crate::state::CurrentWorktreeFingerprint,
    after: &crate::state::CurrentWorktreeFingerprint,
) -> std::result::Result<String, String> {
    let before = before
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("before work check", before.error.as_deref()))?;
    let after = after
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("after work check", after.error.as_deref()))?;

    if before == after {
        Ok(after.to_string())
    } else {
        Err(
            "worktree changed during work check; rerun work check after generated changes settle"
                .to_string(),
        )
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}

fn gate_status(ctx: &RepoContext, plan_id: &str) -> Result<Value> {
    let mut gates = Vec::new();
    let mut missing_required = Vec::new();
    let mut failed_required = Vec::new();
    let mut stale_required = Vec::new();
    let mut unknown_required = Vec::new();
    let mut unsupported_required = Vec::new();
    let current_fingerprint = current_worktree_fingerprint(ctx);

    for gate in ctx.work_gates() {
        let status = gate_status_value(ctx, plan_id, &gate, &current_fingerprint)?;
        collect_required_gate_failure(
            &gate,
            &status,
            &mut missing_required,
            &mut failed_required,
            &mut stale_required,
            &mut unknown_required,
            &mut unsupported_required,
        );
        gates.push(status);
    }

    let ok = missing_required.is_empty()
        && failed_required.is_empty()
        && stale_required.is_empty()
        && unknown_required.is_empty()
        && unsupported_required.is_empty();

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "overall": if ok { "passed" } else { "blocked" },
        "gates": gates,
        "missing_required": missing_required,
        "failed_required": failed_required,
        "stale_required": stale_required,
        "unknown_required": unknown_required,
        "unsupported_required": unsupported_required,
    }))
}

fn gate_status_value(
    ctx: &RepoContext,
    plan_id: &str,
    gate: &WorkGateConfig,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> Result<Value> {
    match gate.kind.as_str() {
        "check" => {
            let tool_name = gate
                .tool
                .as_deref()
                .ok_or_else(|| anyhow!("Work gate '{}' is missing tool", gate.id))?;
            validate_check_tool(ctx, tool_name, "Work gate")?;
            let receipt = latest_plan_tool_receipt(ctx, plan_id, tool_name)?;
            let freshness_receipt = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => {
                    latest_plan_work_check_receipt_for_tool(
                        ctx,
                        plan_id,
                        tool_name,
                        receipt.ended_at_ms,
                    )?
                    .or_else(|| Some(receipt.clone()))
                }
                _ => receipt.clone(),
            };
            let freshness = gate_freshness(&freshness_receipt, current_fingerprint);
            let status = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => "passed",
                Some(_) => "failed",
                None => "missing",
            };
            let status = if status == "passed" && freshness != "fresh" {
                freshness
            } else {
                status
            };

            Ok(json!({
                "id": gate.id,
                "kind": gate.kind,
                "required": gate.required,
                "tool": tool_name,
                "status": status,
                "receipt_id": receipt.as_ref().map(|receipt| receipt.receipt_id.as_str()),
                "freshness_receipt_id": freshness_receipt
                    .as_ref()
                    .map(|receipt| receipt.receipt_id.as_str()),
                "exit_status": receipt.as_ref().map(|receipt| receipt.exit_status),
                "ended_at_ms": receipt.as_ref().map(|receipt| receipt.ended_at_ms),
                "freshness": freshness,
                "receipt_worktree_fingerprint_error": freshness_receipt
                    .as_ref()
                    .and_then(|receipt| receipt.worktree_fingerprint_error.as_deref()),
                "current_worktree_fingerprint_error": current_fingerprint.error.as_deref(),
            }))
        }
        other => Ok(json!({
            "id": gate.id,
            "kind": other,
            "required": gate.required,
            "status": "unsupported",
        })),
    }
}

fn collect_required_gate_failure(
    gate: &WorkGateConfig,
    status: &Value,
    missing_required: &mut Vec<String>,
    failed_required: &mut Vec<String>,
    stale_required: &mut Vec<String>,
    unknown_required: &mut Vec<String>,
    unsupported_required: &mut Vec<String>,
) {
    if !gate.required {
        return;
    }

    match status["status"].as_str() {
        Some("passed") => {}
        Some("missing") => missing_required.push(gate.id.clone()),
        Some("failed") => failed_required.push(gate.id.clone()),
        Some("stale") => stale_required.push(gate.id.clone()),
        Some("unknown") => unknown_required.push(gate.id.clone()),
        _ => unsupported_required.push(gate.id.clone()),
    }
}

fn ensure_required_gates_passed(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    let status = gate_status(ctx, plan_id)?;
    if status["overall"] == "passed" {
        return Ok(());
    }

    let missing = gate_list(&status, "missing_required");
    let failed = gate_list(&status, "failed_required");
    let stale = gate_list(&status, "stale_required");
    let unknown = gate_list(&status, "unknown_required");
    let unsupported = gate_list(&status, "unsupported_required");
    let fingerprint_errors = gate_fingerprint_errors(&status);
    let fingerprint_error_details = if fingerprint_errors.is_empty() {
        String::new()
    } else {
        format!(" Fingerprint errors: [{}].", fingerprint_errors.join("; "))
    };

    bail!(
        "Required work gates are not satisfied for plan {plan_id}. Missing: [{}]. Failed: [{}]. Stale: [{}]. Unknown: [{}]. Unsupported: [{}].{} Run `scripts/jig work gates --plan-id {plan_id}` for details.",
        missing.join(", "),
        failed.join(", "),
        stale.join(", "),
        unknown.join(", "),
        unsupported.join(", "),
        fingerprint_error_details,
    )
}

fn gate_freshness(
    receipt: &Option<crate::state::ToolReceiptStatus>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> &'static str {
    let Some(receipt) = receipt else {
        return "missing";
    };
    let Some(receipt_fingerprint) = receipt.worktree_fingerprint.as_deref() else {
        return "unknown";
    };
    let Some(current_fingerprint) = current_fingerprint.fingerprint.as_deref() else {
        return "unknown";
    };
    if receipt_fingerprint == current_fingerprint {
        "fresh"
    } else {
        "stale"
    }
}

fn gate_list(status: &Value, key: &str) -> Vec<String> {
    status[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn gate_fingerprint_errors(status: &Value) -> Vec<String> {
    status["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|gate| {
            let id = gate["id"].as_str()?;
            let current = gate["current_worktree_fingerprint_error"].as_str();
            let receipt = gate["receipt_worktree_fingerprint_error"].as_str();
            match (current, receipt) {
                (None, None) => None,
                (Some(current), None) => Some(format!("{id}: current={}", concise_error(current))),
                (None, Some(receipt)) => Some(format!("{id}: receipt={}", concise_error(receipt))),
                (Some(current), Some(receipt)) => Some(format!(
                    "{id}: current={}, receipt={}",
                    concise_error(current),
                    concise_error(receipt)
                )),
            }
        })
        .collect()
}

fn concise_error(error: &str) -> String {
    let one_line = error.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_ERROR_CHARS: usize = 240;
    if one_line.chars().count() <= MAX_ERROR_CHARS {
        return one_line;
    }

    let mut truncated = one_line.chars().take(MAX_ERROR_CHARS).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn validate_check_tool(ctx: &RepoContext, name: &str, label: &str) -> Result<()> {
    let tool = ctx
        .tool_spec(name)
        .ok_or_else(|| anyhow!("{label} is not declared in .agent/jig-contract.json: {name}"))?;
    if !tool_defs::is_make_tool(tool) {
        bail!("{label} is not a make-backed tool: {name}");
    }
    if tool_defs::make_tool_requires_name(tool) {
        bail!("{label} requires an argument and cannot run as a configured gate: {name}");
    }
    Ok(())
}
