use anyhow::Result;
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::tool;

pub(crate) use events::now_ms;
use events::{DecisionRecord, append_jsonl, ensure_state_layout, new_id};
#[cfg(test)]
use events::{PlanEvent, ReceiptRecord, read_jsonl, truncate};
pub(crate) use plans::{
    PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, ensure_plan_is_open, plans_append,
    plans_close, plans_open,
};
pub(crate) use receipts::{
    CurrentWorktreeFingerprint, DEFAULT_RECEIPTS_LIMIT, ToolReceiptStatus,
    current_worktree_fingerprint, latest_plan_tool_receipt,
    latest_plan_work_check_receipt_for_tool,
};
pub(crate) use receipts::{ReceiptInput, ReceiptListFilter, receipts_list, record_receipt};
use receipts::{StateToolReceipt, record_successful_state_tool};
#[cfg(test)]
use sessions::build_summary;
pub(crate) use sessions::current_session;
pub(crate) use sessions::{SessionEndRequest, session_end, session_start, state_summary};

mod events;
mod plans;
mod receipts;
mod sessions;

#[derive(Deserialize)]
pub(crate) struct DecisionAddRequest {
    pub(crate) title: String,
    pub(crate) selected_option: String,
    pub(crate) rationale: String,
    #[serde(default, deserialize_with = "null_as_default")]
    pub(crate) alternatives: Vec<String>,
    pub(crate) plan_id: Option<String>,
}

pub(crate) fn decisions_add(ctx: &RepoContext, request: DecisionAddRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let record = DecisionRecord {
        id: new_id("decision"),
        session_id: current_session(ctx)?,
        plan_id: request.plan_id.clone(),
        title: request.title.clone(),
        selected_option: request.selected_option.clone(),
        rationale: request.rationale.clone(),
        alternatives: request.alternatives.clone(),
        timestamp_ms: now_ms(),
    };
    append_jsonl(&ctx.state_file("decisions.jsonl"), &record)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::DECISIONS_ADD,
            args: json!({
                "title": request.title,
                "selected_option": request.selected_option,
                "plan_id": request.plan_id,
            }),
            started_at_ms: record.timestamp_ms,
            plan_id: record.plan_id.clone(),
            session_override: record.session_id.clone(),
        },
    )?;

    Ok(json!({
        "ok": true,
        "decision_id": record.id,
        "receipt_id": receipt_id,
    }))
}

fn null_as_default<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests;
