use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use super::*;

fn write_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.yml"),
        r#"_src_path: '/tmp/template'
_commit: 'abc123'
repo_name: 'demo'
default_branch: 'main'
jig_version: '0.1.0'
work:
  gates:
    - id: custom
      kind: check
      tool: jig.custom_check
"#,
    )
    .unwrap();
    fs::write(
        root.join("Makefile"),
        "custom-check:\n\t@printf 'manifest target ran\\n'\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["custom-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.custom_check",
                    "kind": "make",
                    "description": "Run make custom-check.",
                    "target": "custom-check"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_mutating_check_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.yml"),
        r#"_src_path: '/tmp/template'
_commit: 'abc123'
repo_name: 'demo'
default_branch: 'main'
jig_version: '0.1.0'
work:
  gates:
    - id: first
      kind: check
      tool: jig.first_check
    - id: mutating
      kind: check
      tool: jig.mutating_check
"#,
    )
    .unwrap();
    fs::write(
        root.join("Makefile"),
        "first-check:\n\t@printf 'first ran\\n'\nmutating-check:\n\t@printf 'generated\\n' > generated.txt\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["first-check", "mutating-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.first_check",
                    "kind": "make",
                    "description": "Run make first-check.",
                    "target": "first-check"
                },
                {
                    "name": "jig.mutating_check",
                    "kind": "make",
                    "description": "Run make mutating-check.",
                    "target": "mutating-check"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn write_failing_check_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.yml"),
        r#"_src_path: '/tmp/template'
_commit: 'abc123'
repo_name: 'demo'
default_branch: 'main'
jig_version: '0.1.0'
work:
  gates:
    - id: custom
      kind: check
      tool: jig.custom_check
"#,
    )
    .unwrap();
    fs::write(
        root.join("Makefile"),
        "custom-check:\n\t@printf 'check failed\\n' >&2\n\t@exit 7\n",
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 1,
            "tool_namespace": "jig",
            "jig_version": "0.1.0",
            "required_make_targets": ["custom-check"],
            "optional_make_targets": [],
            "tools": [
                {
                    "name": "jig.custom_check",
                    "kind": "make",
                    "description": "Run make custom-check.",
                    "target": "custom-check"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
}

fn open_test_plan(ctx: &RepoContext) -> String {
    let plan = crate::state::plans_open(
        ctx,
        crate::state::PlanOpenRequest {
            title: "Test plan".into(),
            body: Some("Test body".into()),
            body_file: None,
        },
    )
    .unwrap();

    plan["plan_id"].as_str().unwrap().to_string()
}

struct TestReceipt<'a> {
    tool_name: &'a str,
    args: Value,
    plan_id: &'a str,
    started_at_ms: u64,
    ended_at_ms: u64,
    worktree_fingerprint: Option<String>,
}

fn record_test_receipt(ctx: &RepoContext, receipt: TestReceipt<'_>) -> String {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: receipt.tool_name,
            args: receipt.args,
            invoked_make_target: None,
            plan_id: Some(receipt.plan_id.to_string()),
            started_at_ms: receipt.started_at_ms,
            ended_at_ms: receipt.ended_at_ms,
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
            collect_git_metadata: false,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: receipt.worktree_fingerprint.map(Ok),
        },
    )
    .unwrap()
}

fn init_git_repo(root: &Path) {
    run_git(root, &["init"]);
    run_git(root, &["config", "user.email", "fixture@example.com"]);
    run_git(root, &["config", "user.name", "Fixture"]);
    run_git(root, &["add", "."]);
    run_git(root, &["commit", "-m", "initial fixture"]);
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn mcp_call_dispatches_make_tool_declared_only_in_manifest() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(&ctx, "jig.custom_check", json!({})).unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["target"], "custom-check");
    assert_eq!(output["result"]["stdout"], "manifest target ran\n");
}

#[test]
fn mcp_work_tools_deserialize_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = call_tool(
        &ctx,
        tool::WORK_START,
        json!({
            "title": "Typed MCP request",
            "body": "Use serde for tool arguments"
        }),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert!(output["plan"]["plan_id"].as_str().is_some());
}

#[test]
fn mcp_work_tools_tolerate_null_optional_defaults() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let check = call_tool(
        &ctx,
        tool::WORK_CHECK,
        json!({
            "plan_id": "plan_1",
            "tools": null
        }),
    )
    .unwrap();
    let receipts = call_tool(
        &ctx,
        tool::WORK_RECEIPTS,
        json!({
            "failed_only": null,
            "limit": null
        }),
    )
    .unwrap();

    assert_eq!(check["ok"], true);
    assert_eq!(receipts["ok"], true);
    assert!(!receipts["receipts"].as_array().unwrap().is_empty());
}

#[test]
fn mcp_work_tools_reject_invalid_typed_arguments() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, tool::WORK_START, json!({ "body": "missing title" })).unwrap_err();
    let error = format!("{error:#}");

    assert!(error.contains("Invalid work tool arguments"));
    assert!(error.contains("missing field `title`"));
}

#[test]
fn make_cli_dispatch_requires_manifest_tool_declaration() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::FmtCheck(crate::cli::ToolOpts { plan_id: None }),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Tool is not declared in .agent/jig-contract.json"));
}

#[test]
fn work_check_runs_configured_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["checks"].as_array().unwrap().len(), 1);
    assert_eq!(output["checks"][0]["tool"], "jig.custom_check");
    assert!(output["checks"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_check_collects_worktree_fingerprint_only_on_batch_receipt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let receipts_text = fs::read_to_string(temp.path().join(".agent/state/receipts.jsonl"))
        .expect("work check should write receipts");
    let receipts = receipts_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let tool_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.custom_check")
        .expect("tool receipt should be recorded");
    let batch_receipt = receipts
        .iter()
        .find(|receipt| receipt["tool_name"] == "jig.work_check")
        .expect("work check batch receipt should be recorded");

    assert!(tool_receipt["worktree_fingerprint"].is_null());
    assert!(batch_receipt["worktree_fingerprint"].as_str().is_some());
    assert_eq!(
        batch_receipt["args"]["receipt_ids"][0],
        tool_receipt["id"].as_str().unwrap()
    );
}

#[test]
fn work_check_marks_batch_fingerprint_unknown_when_checks_mutate_worktree() {
    let temp = tempdir().unwrap();
    write_mutating_check_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["unknown_required"].as_array().unwrap().len(), 2);
    assert_eq!(gates["gates"][0]["status"], "unknown");
    assert!(
        gates["gates"][0]["receipt_worktree_fingerprint_error"]
            .as_str()
            .unwrap()
            .contains("worktree changed during work check")
    );
}

#[test]
fn work_gates_reports_missing_and_passing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let missing = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();
    assert_eq!(missing["overall"], "blocked");
    assert_eq!(missing["gates"][0]["id"], "custom");
    assert_eq!(missing["gates"][0]["status"], "missing");
    assert_eq!(missing["missing_required"][0], "custom");

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let passed = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();
    assert_eq!(passed["overall"], "passed");
    assert_eq!(passed["gates"][0]["status"], "passed");
    assert!(passed["gates"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_finish_rejects_missing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Required work gates are not satisfied"));
    assert!(error.contains("Missing: [custom]"));
}

#[test]
fn work_finish_rejects_unknown_plan_before_checking_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: "plan_missing".into(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    assert!(!error.contains("Required work gates are not satisfied"));
}

#[test]
fn work_finish_allows_passing_required_gates() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id: plan_id.clone(),
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["plan"]["plan_id"], plan_id);
}

#[test]
fn work_gates_reject_stale_required_gate_receipts() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            tools: Vec::new(),
        })),
    )
    .unwrap();
    fs::write(
        temp.path().join("Makefile"),
        "custom-check:\n\t@printf 'changed target ran\\n'\n",
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: plan_id.clone(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "stale");
    assert_eq!(gates["gates"][0]["freshness"], "stale");
    assert_eq!(gates["stale_required"][0], "custom");

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Stale: [custom]"));
}

#[test]
fn work_gates_reject_unknown_required_gate_freshness() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let plan_id = open_test_plan(&ctx);

    dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: plan_id.clone(),
            tools: Vec::new(),
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: plan_id.clone(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "unknown");
    assert_eq!(gates["gates"][0]["freshness"], "unknown");
    assert_eq!(gates["unknown_required"][0], "custom");

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Finish(
            crate::cli::WorkFinishOpts {
                plan_id,
                resolution: Some("done".into()),
                outcome: Some("success".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Unknown: [custom]"));
}

#[test]
fn work_gates_use_direct_receipt_when_prior_batch_ended_in_same_millisecond() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 200,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );
    let direct_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 200,
            ended_at_ms: 200,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], direct_receipt_id);
}

#[test]
fn work_gates_use_legacy_batch_receipt_without_receipt_ids() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let legacy_batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({ "plan_id": "plan_1", "tools": ["jig.custom_check"] }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(
        gates["gates"][0]["freshness_receipt_id"],
        legacy_batch_receipt_id
    );
}

#[test]
fn work_gates_use_exact_batch_receipt_id_when_batches_interleave() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    init_git_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let fingerprint = crate::state::current_worktree_fingerprint(&ctx)
        .fingerprint
        .expect("git fixture should produce fingerprint");

    let tool_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: "jig.custom_check",
            args: json!({}),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 110,
            worktree_fingerprint: None,
        },
    );
    let batch_receipt_id = record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": [tool_receipt_id],
            }),
            plan_id: "plan_1",
            started_at_ms: 100,
            ended_at_ms: 120,
            worktree_fingerprint: Some(fingerprint),
        },
    );
    record_test_receipt(
        &ctx,
        TestReceipt {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": "plan_1",
                "tools": ["jig.custom_check"],
                "receipt_ids": ["receipt_other_tool"],
            }),
            plan_id: "plan_1",
            started_at_ms: 90,
            ended_at_ms: 130,
            worktree_fingerprint: Some("stale-fingerprint".into()),
        },
    );

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "passed");
    assert_eq!(gates["gates"][0]["status"], "passed");
    assert_eq!(gates["gates"][0]["freshness"], "fresh");
    assert_eq!(gates["gates"][0]["freshness_receipt_id"], batch_receipt_id);
}

#[test]
fn work_gates_keep_failed_checks_failed_when_freshness_is_unknown() {
    let temp = tempdir().unwrap();
    write_failing_check_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("jig.custom_check failed with status 2"));

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
        })),
    )
    .unwrap();

    assert_eq!(gates["overall"], "blocked");
    assert_eq!(gates["gates"][0]["status"], "failed");
    assert_eq!(gates["gates"][0]["freshness"], "unknown");
    assert_eq!(gates["failed_required"][0], "custom");
}

#[test]
fn old_flat_memory_tool_names_are_not_supported() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = call_tool(&ctx, "jig.session_start", json!({}))
        .unwrap_err()
        .to_string();

    assert!(error.contains("Unsupported tool: jig.session_start"));
}
