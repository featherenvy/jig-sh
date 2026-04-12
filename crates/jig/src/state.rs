use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use fs4::fs_std::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use ulid::Ulid;

use crate::cli::{
    DecisionAddOpts, PlanAppendOpts, PlanCloseOpts, PlanOpenOpts, ReceiptsListOpts, SessionEndOpts,
};
use crate::context::RepoContext;
use crate::git_receipts::{DiffStat, collect_git_receipt_metadata};

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct SessionEvent {
    id: String,
    session_id: String,
    event: String,
    timestamp_ms: u64,
    outcome: Option<String>,
    summary: Option<Value>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct PlanEvent {
    id: String,
    plan_id: String,
    event: String,
    timestamp_ms: u64,
    title: Option<String>,
    body_path: Option<String>,
    resolution: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct ReceiptRecord {
    id: String,
    session_id: Option<String>,
    plan_id: Option<String>,
    tool_name: String,
    args: Value,
    invoked_make_target: Option<String>,
    started_at_ms: u64,
    ended_at_ms: u64,
    exit_status: i32,
    stdout_preview: String,
    stderr_preview: String,
    changed_paths: Vec<String>,
    diff_stat: DiffStat,
    #[serde(default)]
    git_status_error: Option<String>,
    #[serde(default)]
    git_diff_stat_error: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
struct DecisionRecord {
    id: String,
    session_id: Option<String>,
    plan_id: Option<String>,
    title: String,
    selected_option: String,
    rationale: String,
    alternatives: Vec<String>,
    timestamp_ms: u64,
}

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
}

pub(crate) fn session_start(ctx: &RepoContext) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let session_id = new_id("session");
    let summary = build_summary(ctx)?;
    let event = SessionEvent {
        id: new_id("session-event"),
        session_id: session_id.clone(),
        event: "start".into(),
        timestamp_ms: now_ms(),
        outcome: None,
        summary: Some(summary.clone()),
    };
    append_jsonl(&ctx.state_file("sessions.jsonl"), &event)?;
    write_current_session(ctx, Some(&session_id))?;

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.session_start",
            args: json!({}),
            invoked_make_target: None,
            plan_id: None,
            started_at_ms: event.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: Some(session_id.clone()),
        },
    )?;

    Ok(json!({
        "ok": true,
        "session_id": session_id,
        "summary": summary,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn session_end(ctx: &RepoContext, opts: SessionEndOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let session_id = match opts.session_id {
        Some(id) => id,
        None => current_session(ctx)?.ok_or_else(|| anyhow!("No active session."))?,
    };
    let event = SessionEvent {
        id: new_id("session-event"),
        session_id: session_id.clone(),
        event: "end".into(),
        timestamp_ms: now_ms(),
        outcome: opts.outcome.clone(),
        summary: None,
    };
    append_jsonl(&ctx.state_file("sessions.jsonl"), &event)?;
    if current_session(ctx)?.as_deref() == Some(session_id.as_str()) {
        write_current_session(ctx, None)?;
    }

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.session_end",
            args: json!({
                "session_id": session_id,
                "outcome": opts.outcome,
            }),
            invoked_make_target: None,
            plan_id: None,
            started_at_ms: event.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: Some(event.session_id.clone()),
        },
    )?;

    Ok(json!({
        "ok": true,
        "session_id": event.session_id,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_open(ctx: &RepoContext, opts: PlanOpenOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let plan_id = new_id("plan");
    let body = plan_body(opts.body, opts.body_file)?;
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
        title: Some(opts.title.clone()),
        body_path: Some(rel_path(ctx.root(), &plan_path)?),
        resolution: None,
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.plans_open",
            args: json!({ "title": opts.title }),
            invoked_make_target: None,
            plan_id: Some(plan_id.clone()),
            started_at_ms: event.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
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

pub(crate) fn plans_append(ctx: &RepoContext, opts: PlanAppendOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let body = plan_body(opts.body, opts.body_file)?;
    let plan_path = ctx.plan_body_path(&opts.plan_id);
    append_text(&plan_path, format!("\n\n{body}").as_bytes())?;

    let event = PlanEvent {
        id: new_id("plan-event"),
        plan_id: opts.plan_id.clone(),
        event: "append".into(),
        timestamp_ms: now_ms(),
        title: None,
        body_path: Some(rel_path(ctx.root(), &plan_path)?),
        resolution: None,
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.plans_append",
            args: json!({ "plan_id": opts.plan_id }),
            invoked_make_target: None,
            plan_id: Some(event.plan_id.clone()),
            started_at_ms: event.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn plans_close(ctx: &RepoContext, opts: PlanCloseOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let event = PlanEvent {
        id: new_id("plan-event"),
        plan_id: opts.plan_id.clone(),
        event: "close".into(),
        timestamp_ms: now_ms(),
        title: None,
        body_path: None,
        resolution: opts.resolution.clone(),
    };
    append_jsonl(&ctx.state_file("plans.jsonl"), &event)?;

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.plans_close",
            args: json!({
                "plan_id": opts.plan_id,
                "resolution": opts.resolution,
            }),
            invoked_make_target: None,
            plan_id: Some(event.plan_id.clone()),
            started_at_ms: event.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": event.plan_id,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn receipts_list(ctx: &RepoContext, opts: ReceiptsListOpts) -> Result<Value> {
    ensure_state_layout(ctx)?;
    let mut receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?;
    if let Some(session_id) = &opts.session_id {
        receipts.retain(|receipt| receipt.session_id.as_ref() == Some(session_id));
    }
    if let Some(plan_id) = &opts.plan_id {
        receipts.retain(|receipt| receipt.plan_id.as_ref() == Some(plan_id));
    }
    receipts.reverse();
    receipts.truncate(opts.limit);

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.receipts_list",
            args: json!({
                "session_id": opts.session_id,
                "plan_id": opts.plan_id,
                "limit": opts.limit,
            }),
            invoked_make_target: None,
            plan_id: None,
            started_at_ms: now_ms(),
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
        },
    )?;

    Ok(json!({
        "ok": true,
        "receipts": receipts,
        "receipt_id": receipt_id,
    }))
}

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

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: "jig.decisions_add",
            args: json!({
                "title": opts.title,
                "selected_option": opts.selected_option,
                "plan_id": opts.plan_id,
            }),
            invoked_make_target: None,
            plan_id: record.plan_id.clone(),
            started_at_ms: record.timestamp_ms,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: record.session_id.clone(),
        },
    )?;

    Ok(json!({
        "ok": true,
        "decision_id": record.id,
        "receipt_id": receipt_id,
    }))
}

pub(crate) fn record_receipt(ctx: &RepoContext, input: ReceiptInput<'_>) -> Result<String> {
    ensure_state_layout(ctx)?;
    let git_metadata = collect_git_receipt_metadata(ctx.root());
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

pub(crate) fn ensure_state_layout(ctx: &RepoContext) -> Result<()> {
    fs::create_dir_all(ctx.state_dir())?;
    fs::create_dir_all(ctx.root().join(".agent/plans"))?;
    if let Some(parent) = ctx.current_session_path().parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn current_session(ctx: &RepoContext) -> Result<Option<String>> {
    let path = ctx.current_session_path();
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

fn write_current_session(ctx: &RepoContext, session_id: Option<&str>) -> Result<()> {
    let path = ctx.current_session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match session_id {
        Some(value) => fs::write(path, format!("{value}\n"))?,
        None => {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

fn build_summary(ctx: &RepoContext) -> Result<Value> {
    let sessions = read_jsonl::<SessionEvent>(&ctx.state_file("sessions.jsonl"))?;
    let plans = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))?;
    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))?;
    let decisions = read_jsonl::<DecisionRecord>(&ctx.state_file("decisions.jsonl"))?;

    let open_plans = open_plans(&plans);

    let recent_receipts = receipts
        .into_iter()
        .rev()
        .take(5)
        .map(|receipt| {
            json!({
                "id": receipt.id,
                "tool_name": receipt.tool_name,
                "exit_status": receipt.exit_status,
            })
        })
        .collect::<Vec<_>>();

    let recent_decisions = decisions
        .into_iter()
        .rev()
        .take(5)
        .map(|decision| {
            json!({
                "id": decision.id,
                "title": decision.title,
                "selected_option": decision.selected_option,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "repo_name": ctx.repo_name(),
        "default_branch": ctx.default_branch(),
        "source_commit": ctx.source_commit(),
        "source_path": ctx.source_path(),
        "recent_sessions": sessions.into_iter().rev().take(3).collect::<Vec<_>>(),
        "open_plans": open_plans,
        "recent_receipts": recent_receipts,
        "recent_decisions": recent_decisions,
    }))
}

fn open_plans(events: &[PlanEvent]) -> Vec<Value> {
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

fn append_jsonl<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.lock_exclusive()?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    file.unlock()?;
    Ok(())
}

fn append_text(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    file.lock_exclusive()?;
    file.write_all(content)?;
    file.sync_data()?;
    file.unlock()?;
    Ok(())
}

fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut items = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str(&line).with_context(|| {
            format!(
                "Failed to parse JSONL record {} in {}",
                index + 1,
                path.display()
            )
        })?;
        items.push(value);
    }
    Ok(items)
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn truncate(value: &str) -> String {
    const LIMIT: usize = 4000;
    if value.len() <= LIMIT {
        value.to_string()
    } else {
        let mut end = LIMIT;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Ulid::new())
}

fn plan_body(body: Option<String>, body_file: Option<PathBuf>) -> Result<String> {
    match (body, body_file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => fs::read_to_string(path).context("Failed to read plan body file"),
        (None, None) => Ok(String::from("# Plan\n")),
        (Some(_), Some(_)) => bail!("Provide either --body or --body-file, not both."),
    }
}

fn rel_path(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)
        .with_context(|| format!("{} is not under {}", path.display(), root.display()))?
        .display()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn write_fixture_repo(root: &Path) {
        fs::create_dir_all(root.join(".agent")).unwrap();
        fs::write(
            root.join(".jig.yml"),
            r#"_src_path: '/tmp/template'
_commit: 'abc123'
repo_name: 'demo'
default_branch: 'main'
jig_version: '0.1.0'
"#,
        )
        .unwrap();
        fs::write(
            root.join(".agent/jig-contract.json"),
            serde_json::to_string_pretty(&json!({
                "contract_version": 1,
                "memory_schema_version": 1,
                "tool_namespace": "jig",
                "jig_version": "0.1.0",
                "required_make_targets": ["fmt-check"],
                "optional_make_targets": [],
                "tools": [],
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn appends_jsonl_records() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        append_jsonl(&path, &json!({ "id": 1 })).unwrap();
        append_jsonl(&path, &json!({ "id": 2 })).unwrap();

        let items: Vec<Value> = read_jsonl(&path).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["id"], 1);
        assert_eq!(items[1]["id"], 2);
    }

    #[test]
    fn session_summary_includes_open_plans() {
        let temp = tempdir().unwrap();
        write_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();
        ensure_state_layout(&ctx).unwrap();
        append_jsonl(
            &ctx.state_file("plans.jsonl"),
            &PlanEvent {
                id: "1".into(),
                plan_id: "plan_1".into(),
                event: "open".into(),
                timestamp_ms: 1,
                title: Some("Example".into()),
                body_path: Some(".agent/plans/plan_1.md".into()),
                resolution: None,
            },
        )
        .unwrap();

        let summary = build_summary(&ctx).unwrap();
        assert_eq!(summary["open_plans"][0]["plan_id"], "plan_1");
    }

    #[test]
    fn truncate_handles_multibyte_boundaries() {
        let value = format!("{}{}", "a".repeat(3999), "é");
        let truncated = truncate(&value);

        assert!(truncated.ends_with('…'));
        assert!(truncated.starts_with(&"a".repeat(3999)));
        assert_eq!(truncated.chars().last(), Some('…'));
    }

    #[test]
    fn plans_append_serializes_concurrent_writers() {
        let temp = tempdir().unwrap();
        write_fixture_repo(temp.path());
        let ctx = RepoContext::load_from(temp.path()).unwrap();

        plans_open(
            &ctx,
            PlanOpenOpts {
                title: "Concurrent plan".into(),
                body: Some("Initial body".into()),
                body_file: None,
            },
        )
        .unwrap();

        let ctx_a = ctx.clone();
        let ctx_b = ctx.clone();
        let plan_id = read_jsonl::<PlanEvent>(&ctx.state_file("plans.jsonl"))
            .unwrap()
            .into_iter()
            .find(|event| event.event == "open")
            .unwrap()
            .plan_id;

        let plan_id_a = plan_id.clone();
        let plan_id_b = plan_id.clone();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                plans_append(
                    &ctx_a,
                    PlanAppendOpts {
                        plan_id: plan_id_a,
                        body: Some("First append".into()),
                        body_file: None,
                    },
                )
                .unwrap();
            });
            scope.spawn(|| {
                plans_append(
                    &ctx_b,
                    PlanAppendOpts {
                        plan_id: plan_id_b,
                        body: Some("Second append".into()),
                        body_file: None,
                    },
                )
                .unwrap();
            });
        });

        let body = fs::read_to_string(ctx.plan_body_path(&plan_id)).unwrap();
        assert!(body.contains("Initial body"));
        assert!(body.contains("First append"));
        assert!(body.contains("Second append"));
    }
}
