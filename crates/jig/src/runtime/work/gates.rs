use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::command::{WorkEvidenceRequest, WorkGatesRequest};
use crate::context::{RepoContext, WorkGateConfig};
use crate::state::{
    PlanStatus, ToolReceiptStatus, current_worktree_fingerprint, ensure_plan_exists,
    latest_plan_tool_receipt, latest_plan_work_check_receipt_for_tool, open_plan_summaries,
    plan_status,
};

use super::tools::validate_check_tool;

const MAX_GATE_CHANGED_PATHS: usize = 100;

pub(super) fn gates(ctx: &RepoContext, opts: WorkGatesRequest) -> Result<Value> {
    ensure_plan_exists(ctx, &opts.plan_id)?;
    gate_status(ctx, &opts.plan_id)
}

pub(super) fn evidence(ctx: &RepoContext, opts: WorkEvidenceRequest) -> Result<Value> {
    let plan_id = resolve_evidence_plan_id(ctx, opts.plan_id)?;
    let mut status = gate_status(ctx, &plan_id)?;
    let latest = latest_passing_gates(&status);
    let object = status
        .as_object_mut()
        .ok_or_else(|| anyhow!("work gate status was not a JSON object"))?;
    object.insert("command".into(), json!("work evidence"));
    object.insert("latest_passing_gates".into(), json!(latest));
    Ok(status)
}

pub(super) fn ensure_required_gates_passed(ctx: &RepoContext, plan_id: &str) -> Result<()> {
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

fn gate_status(ctx: &RepoContext, plan_id: &str) -> Result<Value> {
    let plan_state = match plan_status(ctx, plan_id)? {
        Some(PlanStatus::Open) => "open",
        Some(PlanStatus::Closed) => "closed",
        None => bail!("Plan not found: {plan_id}"),
    };
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

    let gates_ok = missing_required.is_empty()
        && failed_required.is_empty()
        && stale_required.is_empty()
        && unknown_required.is_empty()
        && unsupported_required.is_empty();

    Ok(json!({
        "ok": true,
        "gates_ok": gates_ok,
        "plan_id": plan_id,
        "plan_state": plan_state,
        "overall": if gates_ok { "passed" } else { "blocked" },
        "current_worktree_fingerprint": current_fingerprint.fingerprint.as_deref(),
        "current_worktree_fingerprint_error": current_fingerprint.error.as_deref(),
        "gates": gates,
        "missing_required": missing_required,
        "failed_required": failed_required,
        "stale_required": stale_required,
        "unknown_required": unknown_required,
        "unsupported_required": unsupported_required,
    }))
}

fn resolve_evidence_plan_id(ctx: &RepoContext, requested: Option<String>) -> Result<String> {
    if let Some(plan_id) = requested {
        ensure_plan_exists(ctx, &plan_id)?;
        return Ok(plan_id);
    }

    let open_plans = open_plan_summaries(ctx)?;
    match open_plans.as_slice() {
        [plan] => plan["plan_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Open plan summary did not include a plan id")),
        [] => bail!(
            "No open work plans. Run `scripts/jig work status --summary` to find recent plan ids, then pass --plan-id to inspect a closed or specific plan."
        ),
        _ => bail!("Multiple open work plans. Pass --plan-id to choose which plan to inspect."),
    }
}

fn latest_passing_gates(status: &Value) -> Vec<Value> {
    let mut latest = BTreeMap::<String, (u64, Value)>::new();
    let gates = status["gates"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for gate in gates {
        if gate["exit_status"].as_i64() != Some(0) {
            continue;
        }
        let Some(tool) = gate["tool"].as_str() else {
            // Only check gates can pass today, and they always include a tool.
            // Skip malformed future gate payloads instead of coalescing them.
            continue;
        };
        let tool = tool.to_string();
        let gate_id = gate["id"].as_str().unwrap_or("<unknown>").to_string();
        let ended_at_ms = gate["ended_at_ms"].as_u64().unwrap_or(0);
        let value = json!({
            "tool": &tool,
            "gate_id": gate["id"],
            "status": gate["status"],
            "receipt_id": gate["receipt_id"],
            "freshness_receipt_id": gate["freshness_receipt_id"],
            "matches_current_worktree": gate["freshness"].as_str() == Some("fresh"),
            "freshness": gate["freshness"],
            "freshness_reason": gate["freshness_reason"],
            "changed_paths": gate["changed_paths"],
            "changed_path_count": gate["changed_path_count"],
            "changed_paths_truncated": gate["changed_paths_truncated"],
            "diff_summary": gate["diff_summary"],
            "ended_at_ms": ended_at_ms,
        });
        match latest.get(&tool) {
            Some((existing_ended_at_ms, _)) if *existing_ended_at_ms > ended_at_ms => {}
            Some((existing_ended_at_ms, existing))
                if *existing_ended_at_ms == ended_at_ms
                    && existing["gate_id"].as_str().unwrap_or("") >= gate_id.as_str() => {}
            // Replace when this receipt is newer, or when the timestamp ties
            // and the gate id sorts after the current winner.
            _ => {
                latest.insert(tool, (ended_at_ms, value));
            }
        }
    }
    latest.into_values().map(|(_, value)| value).collect()
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
                    // Freshness is anchored to the batch work-check receipt
                    // when available, since that receipt captures the
                    // before/after worktree fingerprint for the gate run.
                    latest_plan_work_check_receipt_for_tool(
                        ctx,
                        plan_id,
                        tool_name,
                        &receipt.receipt_id,
                    )?
                    .or_else(|| Some(receipt.clone()))
                }
                _ => receipt.clone(),
            };
            let freshness = gate_freshness(&freshness_receipt, current_fingerprint);
            let freshness_reason =
                gate_freshness_reason(&freshness_receipt, current_fingerprint, freshness);
            let (changed_paths, changed_path_count, changed_paths_truncated) =
                gate_changed_paths(freshness_receipt.as_ref());
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
                "freshness_reason": freshness_reason,
                "changed_paths": changed_paths,
                "changed_path_count": changed_path_count,
                "changed_paths_truncated": changed_paths_truncated,
                "diff_summary": freshness_receipt
                    .as_ref()
                    .map(|receipt| receipt.diff_summary.as_str()),
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

fn gate_changed_paths(receipt: Option<&ToolReceiptStatus>) -> (Vec<String>, usize, bool) {
    let Some(receipt) = receipt else {
        return (Vec::new(), 0, false);
    };
    let total = receipt.changed_paths.len();
    let paths = receipt
        .changed_paths
        .iter()
        .take(MAX_GATE_CHANGED_PATHS)
        .cloned()
        .collect::<Vec<_>>();
    (paths, total, total > MAX_GATE_CHANGED_PATHS)
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
        Some("unsupported") => unsupported_required.push(unsupported_gate_label(gate, status)),
        _ => unsupported_required.push(unsupported_gate_label(gate, status)),
    }
}

fn unsupported_gate_label(gate: &WorkGateConfig, status: &Value) -> String {
    status["kind"].as_str().map_or_else(
        || gate.id.clone(),
        |kind| format!("{} (kind: {kind})", gate.id.as_str()),
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

fn gate_freshness_reason(
    receipt: &Option<crate::state::ToolReceiptStatus>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
    freshness: &str,
) -> &'static str {
    match freshness {
        "fresh" => "receipt matches current worktree fingerprint",
        "missing" => "no receipt exists for this gate",
        "stale" => "receipt was recorded for a different worktree fingerprint",
        "unknown" => {
            if receipt
                .as_ref()
                .and_then(|receipt| receipt.worktree_fingerprint.as_deref())
                .is_none()
            {
                "receipt did not record a worktree fingerprint"
            } else if current_fingerprint.fingerprint.is_none() {
                "current worktree fingerprint could not be collected"
            } else {
                "worktree freshness could not be determined"
            }
        }
        _ => "worktree freshness could not be determined",
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

    let mut truncated = one_line
        .chars()
        .take(MAX_ERROR_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::{concise_error, latest_passing_gates};
    use serde_json::json;

    #[test]
    fn concise_error_reserves_room_for_ellipsis() {
        let error = "x".repeat(300);
        let concise = concise_error(&error);

        assert_eq!(concise.chars().count(), 240);
        assert!(concise.ends_with("..."));
    }

    #[test]
    fn latest_passing_gates_uses_gate_id_tie_breaker() {
        let status = json!({
            "gates": [
                {
                    "id": "alpha",
                    "tool": "jig.test",
                    "exit_status": 0,
                    "status": "passed",
                    "receipt_id": "receipt-alpha",
                    "freshness_receipt_id": "receipt-alpha",
                    "freshness": "fresh",
                    "freshness_reason": "receipt matches current worktree fingerprint",
                    "changed_paths": [],
                    "changed_path_count": 0,
                    "changed_paths_truncated": false,
                    "diff_summary": null,
                    "ended_at_ms": 42,
                },
                {
                    "id": "zeta",
                    "tool": "jig.test",
                    "exit_status": 0,
                    "status": "passed",
                    "receipt_id": "receipt-zeta",
                    "freshness_receipt_id": "receipt-zeta",
                    "freshness": "fresh",
                    "freshness_reason": "receipt matches current worktree fingerprint",
                    "changed_paths": [],
                    "changed_path_count": 0,
                    "changed_paths_truncated": false,
                    "diff_summary": null,
                    "ended_at_ms": 42,
                }
            ]
        });

        let latest = latest_passing_gates(&status);

        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0]["gate_id"], "zeta");
        assert_eq!(latest[0]["receipt_id"], "receipt-zeta");
    }
}
