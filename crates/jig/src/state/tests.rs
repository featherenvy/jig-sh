use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;
use crate::context::RepoContext;
use crate::git_receipts::DiffStat;
use crate::tool_defs::tool;

fn write_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
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
        &PlanEvent::open(
            "1".into(),
            "plan_1".into(),
            1,
            "Example".into(),
            Some(".agent/plans/plan_1.md".into()),
        ),
    )
    .unwrap();

    let summary = build_summary(&ctx).unwrap();
    assert_eq!(summary["open_plans"][0]["plan_id"], "plan_1");
}

#[test]
fn legacy_unknown_plan_events_stay_readable() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("plans.jsonl");
    fs::write(
        &path,
        r#"{"id":"1","plan_id":"plan_1","event":"pause","timestamp_ms":1}
"#,
    )
    .unwrap();

    let events = read_jsonl::<PlanEvent>(&path).unwrap();

    assert_eq!(events.len(), 1);
    assert!(super::plans::open_plans(&events).is_empty());
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
        PlanOpenRequest {
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
        .find(PlanEvent::is_open)
        .unwrap()
        .plan_id()
        .to_string();

    let plan_id_a = plan_id.clone();
    let plan_id_b = plan_id.clone();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            plans_append(
                &ctx_a,
                PlanAppendRequest {
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
                PlanAppendRequest {
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

#[test]
fn plans_close_rejects_unknown_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: "plan_missing".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
}

#[test]
fn plans_close_rejects_already_closed_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Close once".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();

    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done again".into()),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(&format!("Plan is already closed: {plan_id}")));
}

#[test]
fn plans_append_rejects_closed_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Append after close".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id: plan_id.clone(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = plans_append(
        &ctx,
        PlanAppendRequest {
            plan_id: plan_id.clone(),
            body: Some("late append".into()),
            body_file: None,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains(&format!("Plan is already closed: {plan_id}")));
}

#[test]
fn structured_work_keeps_legacy_state_receipt_tool_names() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let plan = plans_open(
        &ctx,
        PlanOpenRequest {
            title: "Receipt compatibility".into(),
            body: Some("Initial body".into()),
            body_file: None,
        },
    )
    .unwrap();
    let plan_id = plan["plan_id"].as_str().unwrap().to_string();
    plans_append(
        &ctx,
        PlanAppendRequest {
            plan_id: plan_id.clone(),
            body: Some("Append body".into()),
            body_file: None,
        },
    )
    .unwrap();
    decisions_add(
        &ctx,
        DecisionAddRequest {
            title: "Decision".into(),
            selected_option: "Keep compatibility".into(),
            rationale: "Receipt filters depend on historical tool names.".into(),
            alternatives: vec!["Rename receipts".into()],
            plan_id: Some(plan_id.clone()),
        },
    )
    .unwrap();
    plans_close(
        &ctx,
        PlanCloseRequest {
            plan_id,
            resolution: Some("done".into()),
        },
    )
    .unwrap();
    session_end(
        &ctx,
        SessionEndRequest {
            session_id: None,
            outcome: Some("done".into()),
        },
    )
    .unwrap();

    let tool_names = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl"))
        .unwrap()
        .into_iter()
        .map(|receipt| receipt.tool_name)
        .collect::<Vec<_>>();

    assert!(tool_names.contains(&tool::SESSION_START.to_string()));
    assert!(tool_names.contains(&tool::PLANS_OPEN.to_string()));
    assert!(tool_names.contains(&tool::PLANS_APPEND.to_string()));
    assert!(tool_names.contains(&tool::DECISIONS_ADD.to_string()));
    assert!(tool_names.contains(&tool::PLANS_CLOSE.to_string()));
    assert!(tool_names.contains(&tool::SESSION_END.to_string()));
}

#[test]
fn receipts_list_is_read_only() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = receipts_list(&ctx, receipt_list_filter()).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
}

#[test]
fn receipts_list_filters_by_tool_and_failure_and_adds_diff_summary() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    ensure_state_layout(&ctx).unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record(
            "receipt_failed",
            tool::TEST,
            1,
            DiffStat {
                files: 1,
                insertions: 2,
                deletions: 3,
            },
        ),
    )
    .unwrap();
    append_jsonl(
        &ctx.state_file("receipts.jsonl"),
        &receipt_record("receipt_success", tool::CLIPPY, 0, DiffStat::default()),
    )
    .unwrap();

    let output = receipts_list(
        &ctx,
        ReceiptListFilter {
            tool_name: Some(tool::TEST.into()),
            failed_only: true,
            ..receipt_list_filter()
        },
    )
    .unwrap();
    let receipts = output["receipts"].as_array().unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0]["id"], "receipt_failed");
    assert_eq!(receipts[0]["diff_summary"], "1 file, +2 -3");
}

fn receipt_list_filter() -> ReceiptListFilter {
    ReceiptListFilter {
        session_id: None,
        plan_id: None,
        tool_name: None,
        failed_only: false,
        limit: 20,
    }
}

fn receipt_record(
    id: &str,
    tool_name: &str,
    exit_status: i32,
    diff_stat: DiffStat,
) -> ReceiptRecord {
    ReceiptRecord {
        id: id.into(),
        session_id: Some("session_1".into()),
        plan_id: Some("plan_1".into()),
        tool_name: tool_name.into(),
        args: json!({}),
        invoked_make_target: None,
        invoked_command_key: None,
        started_at_ms: 1,
        ended_at_ms: 2,
        exit_status,
        stdout_preview: String::new(),
        stderr_preview: String::new(),
        changed_paths: Vec::new(),
        diff_stat,
        git_status_error: None,
        git_diff_stat_error: None,
        worktree_fingerprint: None,
        worktree_fingerprint_error: None,
    }
}

#[test]
fn state_summary_is_read_only_and_counts_state_records() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();
    let before = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();

    let output = state_summary(&ctx).unwrap();

    let after = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    assert_eq!(before.len(), after.len());
    assert!(output.get("receipt_id").is_none());
    assert_eq!(output["ok"], true);
    assert_eq!(output["counts"]["sessions"], 1);
    assert_eq!(output["counts"]["plans"], 0);
    assert_eq!(output["counts"]["receipts"], 1);
    assert_eq!(output["counts"]["failed_receipts"], 0);
    assert_eq!(
        output["recent_receipts"][0]["tool_name"],
        tool::SESSION_START
    );
}

#[test]
fn state_tool_receipts_skip_git_metadata_collection() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    session_start(&ctx).unwrap();

    let receipts = read_jsonl::<ReceiptRecord>(&ctx.state_file("receipts.jsonl")).unwrap();
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.tool_name == tool::SESSION_START)
        .unwrap();
    assert_eq!(receipt.args["operation"], "session_start");
    assert!(receipt.changed_paths.is_empty());
    assert_eq!(receipt.diff_stat.files, 0);
    assert!(receipt.git_status_error.is_none());
    assert!(receipt.git_diff_stat_error.is_none());
}
