use anyhow::Result;
use serde_json::{Value, json};

use crate::command::WorkCheckRequest;
use crate::context::RepoContext;
use crate::state::{ReceiptInput, current_worktree_fingerprint, now_ms, record_receipt};
use crate::tool_defs::tool;

use super::super::execute_manifest_tool_without_worktree_fingerprint;
use super::tools::{selected_tools, validate_check_tool};

pub(super) fn check(ctx: &RepoContext, opts: WorkCheckRequest) -> Result<Value> {
    // Closed plans are inspectable through gates/evidence, but checks append
    // fresh receipts and must stay tied to open work.
    crate::state::ensure_plan_is_open(ctx, &opts.plan_id)?;
    check_tools(ctx, &opts.plan_id, selected_tools(ctx, &opts.tools)?)
}

fn check_tools(ctx: &RepoContext, plan_id: &str, tools: Vec<String>) -> Result<Value> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    let mut results = Vec::with_capacity(tools.len());
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;

        results.push(execute_manifest_tool_without_worktree_fingerprint(
            ctx,
            name,
            json!({}),
            Some(plan_id.to_string()),
        )?);
    }
    let receipt_ids = results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
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
                "receipt_ids": receipt_ids,
            }),
            invoked_command_key: None,
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
        Err(format!(
            "worktree changed during work check; before fingerprint {before}, after fingerprint {after}; rerun work check after generated changes settle"
        ))
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}
