use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::DecisionAddOpts;
use crate::context::RepoContext;
use crate::tool_defs::tool;

pub(crate) use events::now_ms;
use events::{DecisionRecord, append_jsonl, ensure_state_layout, new_id};
#[cfg(test)]
use events::{PlanEvent, ReceiptRecord, read_jsonl, truncate};
pub(crate) use plans::{plans_append, plans_close, plans_open};
pub(crate) use receipts::{ReceiptInput, receipts_list, record_receipt};
use receipts::{StateToolReceipt, record_successful_state_tool};
#[cfg(test)]
use sessions::build_summary;
use sessions::current_session;
pub(crate) use sessions::{session_end, session_start, state_summary};

mod events;
mod plans;
mod receipts;
mod sessions;

pub(crate) fn decisions_add(ctx: &RepoContext, opts: DecisionAddOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let record = DecisionRecord {
        id: new_id("decision"),
        session_id: current_session(ctx)?,
        plan_id: opts.plan_id.clone(),
        title: opts.title.clone(),
        selected_option: opts.selected_option.clone(),
        rationale: opts.rationale.clone(),
        alternatives: opts.alternatives.clone(),
        timestamp_ms: now_ms(),
    };
    append_jsonl(&ctx.state_file("decisions.jsonl"), &record)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::DECISIONS_ADD,
            args: json!({
                "title": opts.title,
                "selected_option": opts.selected_option,
                "plan_id": opts.plan_id,
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

#[cfg(test)]
mod tests;
