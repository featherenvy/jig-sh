use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::context::RepoContext;
use crate::tool_defs::{args, tool};

use super::events::{
    PlanEvent, append_jsonl, append_text, ensure_state_layout, new_id, now_ms, rel_path,
};
use super::receipts::{StateToolReceipt, record_successful_state_tool};

pub(crate) struct PlanOpenRequest {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

pub(crate) struct PlanAppendRequest {
    pub(crate) plan_id: String,
    pub(crate) body: Option<String>,
    pub(crate) body_file: Option<PathBuf>,
}

pub(crate) struct PlanCloseRequest {
    pub(crate) plan_id: String,
    pub(crate) resolution: Option<String>,
}

pub(crate) fn plans_open(ctx: &RepoContext, request: PlanOpenRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let plan_id = new_id("plan");
    let body = plan_body(request.body, request.body_file)?;
    let plan_path = ctx.plan_body_path(&plan_id);
    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plan_path, body)?;

    let event = PlanEvent {
        id: new_id("plan-event"),
        plan_id: plan_id.clone(),
        event: "open".into(),
        timestamp_ms: now_ms(),
        title: Some(request.title.clone()),
        body_path: Some(rel_path(ctx.root(), &plan_path)?),
        resolution: None,
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_OPEN,
            args: json!({
                args::OPERATION: "plan_open",
                "title": request.title,
            }),
            started_at_ms: event.timestamp_ms,
            plan_id: Some(plan_id.clone()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "body_path": event.body_path,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_append(ctx: &RepoContext, request: PlanAppendRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let body = plan_body(request.body, request.body_file)?;
    let plan_path = ctx.plan_body_path(&request.plan_id);
    append_text(&plan_path, format!("\n\n{body}").as_bytes())?;

    let event = PlanEvent {
        id: new_id("plan-event"),
        plan_id: request.plan_id.clone(),
        event: "append".into(),
        timestamp_ms: now_ms(),
        title: None,
        body_path: Some(rel_path(ctx.root(), &plan_path)?),
        resolution: None,
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_APPEND,
            args: json!({
                args::OPERATION: "plan_append",
                "plan_id": request.plan_id,
            }),
            started_at_ms: event.timestamp_ms,
            plan_id: Some(event.plan_id.clone()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_close(ctx: &RepoContext, request: PlanCloseRequest) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let event = PlanEvent {
        id: new_id("plan-event"),
        plan_id: request.plan_id.clone(),
        event: "close".into(),
        timestamp_ms: now_ms(),
        title: None,
        body_path: None,
        resolution: request.resolution.clone(),
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_successful_state_tool(
        ctx,
        StateToolReceipt {
            tool_name: tool::PLANS_CLOSE,
            args: json!({
                args::OPERATION: "plan_close",
                "plan_id": request.plan_id,
                "resolution": request.resolution,
            }),
            started_at_ms: event.timestamp_ms,
            plan_id: Some(event.plan_id.clone()),
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id,
        "receipt_id": receipt_id,
    }))
}

pub(super) fn open_plans(events: &[PlanEvent]) -> Vec<Value> {
    let mut closed = HashSet::new();
    let mut opened = BTreeMap::<String, (&str, Option<&str>)>::new();
    for event in events {
        match event.event.as_str() {
            "open" => {
                opened.insert(
                    event.plan_id.clone(),
                    (
                        event.title.as_deref().unwrap_or("Untitled plan"),
                        event.body_path.as_deref(),
                    ),
                );
            }
            "close" => {
                closed.insert(event.plan_id.clone());
            }
            _ => {}
        }
    }

    opened
        .into_iter()
        .filter(|(plan_id, _)| !closed.contains(plan_id))
        .map(|(plan_id, (title, body_path))| {
            json!({
                "plan_id": plan_id,
                "title": title,
                "body_path": body_path,
            })
        })
        .collect()
}

fn plan_body(body: Option<String>, body_file: Option<PathBuf>) -> Result<String> {
    match (body, body_file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(path).context("Failed to read plan body file"),
        (None, None) => Ok(String::from("# Plan\n")),
        (Some(_), Some(_)) => bail!("Provide either --body or --body-file, not both."),
    }
}
