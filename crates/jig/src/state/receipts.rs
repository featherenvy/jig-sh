use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::ReceiptsListOpts;
use crate::context::RepoContext;
use crate::git_receipts::{GitReceiptMetadata, collect_git_receipt_metadata};

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
}

pub(super) struct StateToolReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) started_at_ms: u64,
    pub(super) plan_id: Option<String>,
    pub(super) session_override: Option<String>,
}

pub(crate) fn receipts_list(ctx: &RepoContext, opts: ReceiptsListOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let mut receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?;
    receipts.retain(|receipt| receipt_matches_filters(receipt, &opts));
    receipts.reverse();
    receipts.truncate(opts.limit);

    Ok(json!({
        "ok": true,
        "receipts": receipts,
    }))
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    ensure_state_layout(ctx)?;
    let git_metadata = receipt_git_metadata(ctx, input.collect_git_metadata);
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
        },
    )
}

fn receipt_matches_filters(receipt: &ReceiptRecord, opts: &ReceiptsListOpts) -> bool {
    let session_matches = opts
        .session_id
        .as_ref()
        .is_none_or(|session_id| receipt.session_id.as_ref() == Some(session_id));
    let plan_matches = opts
        .plan_id
        .as_ref()
        .is_none_or(|plan_id| receipt.plan_id.as_ref() == Some(plan_id));

    session_matches && plan_matches
}

fn receipt_git_metadata(ctx: &RepoContext, collect_git_metadata: bool) -> GitReceiptMetadata {
    if collect_git_metadata {
        collect_git_receipt_metadata(ctx.root())
    } else {
        GitReceiptMetadata::default()
    }
}
