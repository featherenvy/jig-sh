use anyhow::Result;
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::git_receipts::{
    GitReceiptMetadata, collect_git_receipt_metadata,
    collect_git_receipt_metadata_without_worktree_fingerprint, repo_worktree_fingerprint,
};
use crate::tool_defs::tool;

use super::events::{
    ReceiptRecord, append_jsonl, ensure_state_layout, new_id, now_ms, read_jsonl, truncate,
};
use super::sessions::current_session;

pub(crate) struct ReceiptInput<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) args: Value,
    pub(crate) invoked_make_target: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) started_at_ms: u64,
    pub(crate) ended_at_ms: u64,
    pub(crate) exit_status: i32,
    pub(crate) stdout: &'a str,
    pub(crate) stderr: &'a str,
    pub(crate) session_override: Option<String>,
    pub(crate) collect_git_metadata: bool,
    pub(crate) collect_worktree_fingerprint: bool,
    pub(crate) worktree_fingerprint_override: Option<std::result::Result<String, String>>,
}

pub(super) struct StateToolReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) started_at_ms: u64,
    pub(super) plan_id: Option<String>,
    pub(super) session_override: Option<String>,
}

pub(crate) struct ReceiptListFilter {
    pub(crate) session_id: Option<String>,
    pub(crate) plan_id: Option<String>,
    pub(crate) tool_name: Option<String>,
    pub(crate) failed_only: bool,
    pub(crate) limit: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ToolReceiptStatus {
    pub(crate) receipt_id: String,
    pub(crate) exit_status: i32,
    pub(crate) ended_at_ms: u64,
    pub(crate) worktree_fingerprint: Option<String>,
    pub(crate) worktree_fingerprint_error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CurrentWorktreeFingerprint {
    pub(crate) fingerprint: Option<String>,
    pub(crate) error: Option<String>,
}

pub(crate) fn receipts_list(ctx: &RepoContext, filter: ReceiptListFilter) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?
        .into_iter()
        .rev()
        .filter(|receipt| receipt_matches_filters(receipt, &filter))
        .take(filter.limit)
        .map(receipt_list_value)
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "ok": true,
        "receipts": receipts,
    }))
}

pub(crate) fn latest_plan_tool_receipt(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
) -> Result<Option<ToolReceiptStatus>> {
    ensure_state_layout(ctx)?;
    Ok(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?
            .into_iter()
            .rev()
            .find(|receipt| {
                receipt.plan_id.as_deref() == Some(plan_id) && receipt.tool_name == tool_name
            })
            .map(|receipt| ToolReceiptStatus {
                receipt_id: receipt.id,
                exit_status: receipt.exit_status,
                ended_at_ms: receipt.ended_at_ms,
                worktree_fingerprint: receipt.worktree_fingerprint,
                worktree_fingerprint_error: receipt.worktree_fingerprint_error,
            }),
    )
}

pub(crate) fn latest_plan_work_check_receipt_for_tool(
    ctx: &RepoContext,
    plan_id: &str,
    tool_name: &str,
    after_ended_at_ms: u64,
) -> Result<Option<ToolReceiptStatus>> {
    ensure_state_layout(ctx)?;
    Ok(
        read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?
            .into_iter()
            .rev()
            .find(|receipt| {
                receipt.plan_id.as_deref() == Some(plan_id)
                    && receipt.tool_name == tool::WORK_CHECK
                    && receipt.exit_status == 0
                    && receipt.ended_at_ms >= after_ended_at_ms
                    && receipt_args_include_tool(receipt, tool_name)
            })
            .map(|receipt| ToolReceiptStatus {
                receipt_id: receipt.id,
                exit_status: receipt.exit_status,
                ended_at_ms: receipt.ended_at_ms,
                worktree_fingerprint: receipt.worktree_fingerprint,
                worktree_fingerprint_error: receipt.worktree_fingerprint_error,
            }),
    )
}

pub(crate) fn current_worktree_fingerprint(ctx: &RepoContext) -> CurrentWorktreeFingerprint {
    match repo_worktree_fingerprint(ctx.root()) {
        Ok(fingerprint) => CurrentWorktreeFingerprint {
            fingerprint: Some(fingerprint),
            error: None,
        },
        Err(error) => CurrentWorktreeFingerprint {
            fingerprint: None,
            error: Some(format!("{error:#}")),
        },
    }
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    ensure_state_layout(ctx)?;
    let mut git_metadata = receipt_git_metadata(
        ctx,
        input.collect_git_metadata,
        input.collect_worktree_fingerprint,
    );
    if let Some(override_result) = input.worktree_fingerprint_override {
        match override_result {
            Ok(fingerprint) => {
                git_metadata.worktree_fingerprint = Some(fingerprint);
                git_metadata.worktree_fingerprint_error = None;
            }
            Err(error) => {
                git_metadata.worktree_fingerprint = None;
                git_metadata.worktree_fingerprint_error = Some(error);
            }
        }
    }
    let receipt = ReceiptRecord {
        id: new_id("receipt"),
        session_id: match input.session_override {
            Some(session_id) => Some(session_id),
            None => current_session(ctx)?,
        },
        plan_id: input.plan_id,
        tool_name: input.tool_name.to_string(),
        args: input.args,
        invoked_make_target: input.invoked_make_target,
        started_at_ms: input.started_at_ms,
        ended_at_ms: input.ended_at_ms,
        exit_status: input.exit_status,
        stdout_preview: truncate(input.stdout),
        stderr_preview: truncate(input.stderr),
        changed_paths: git_metadata.changed_paths,
        diff_stat: git_metadata.diff_stat,
        git_status_error: git_metadata.git_status_error,
        git_diff_stat_error: git_metadata.git_diff_stat_error,
        worktree_fingerprint: git_metadata.worktree_fingerprint,
        worktree_fingerprint_error: git_metadata.worktree_fingerprint_error,
    };
    let receipt_id = receipt.id.clone();
    append_jsonl(&ctx.state_file("receipts.jsonl"), &receipt)?;
    Ok(receipt_id)
}

pub(super) fn record_successful_state_tool(
    ctx: &RepoContext,
    input: StateToolReceipt<'_>,
) -> Result<String> {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: input.tool_name,
            args: input.args,
            invoked_make_target: None,
            plan_id: input.plan_id,
            started_at_ms: input.started_at_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: input.session_override,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: None,
        },
    )
}

fn receipt_matches_filters(receipt: &ReceiptRecord, filter: &ReceiptListFilter) -> bool {
    let session_matches = filter
        .session_id
        .as_ref()
        .is_none_or(|session_id| receipt.session_id.as_ref() == Some(session_id));
    let plan_matches = filter
        .plan_id
        .as_ref()
        .is_none_or(|plan_id| receipt.plan_id.as_ref() == Some(plan_id));
    let tool_matches = filter
        .tool_name
        .as_ref()
        .is_none_or(|tool_name| receipt.tool_name == *tool_name);
    let failure_matches = !filter.failed_only || receipt.exit_status != 0;

    session_matches && plan_matches && tool_matches && failure_matches
}

fn receipt_args_include_tool(receipt: &ReceiptRecord, tool_name: &str) -> bool {
    receipt
        .args
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(|tool| tool.as_str() == Some(tool_name)))
}

fn receipt_list_value(receipt: ReceiptRecord) -> Result<Value> {
    let diff_summary = receipt_diff_summary(&receipt);
    let mut value = serde_json::to_value(receipt)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("diff_summary".to_string(), Value::String(diff_summary));
    }
    Ok(value)
}

pub(super) fn receipt_diff_summary(receipt: &ReceiptRecord) -> String {
    if receipt.git_status_error.is_some() || receipt.git_diff_stat_error.is_some() {
        return "git metadata unavailable".to_string();
    }

    let stat = &receipt.diff_stat;
    if stat.files == 0 && stat.insertions == 0 && stat.deletions == 0 {
        "no changes".to_string()
    } else {
        let file_count = if stat.files == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", stat.files)
        };
        format!("{file_count}, +{} -{}", stat.insertions, stat.deletions)
    }
}

fn receipt_git_metadata(
    ctx: &RepoContext,
    collect_git_metadata: bool,
    collect_worktree_fingerprint: bool,
) -> GitReceiptMetadata {
    if !collect_git_metadata {
        return GitReceiptMetadata::default();
    }

    if collect_worktree_fingerprint {
        collect_git_receipt_metadata(ctx.root())
    } else {
        collect_git_receipt_metadata_without_worktree_fingerprint(ctx.root())
    }
}
