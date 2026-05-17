use super::*;

#[test]
fn make_cli_dispatch_requires_manifest_tool_declaration() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Fmt(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Tool is not declared in .agent/jig-contract.json"));
}

#[test]
fn unavailable_schema_check_explains_disabled_config() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Check(crate::cli::CheckCommand::Schema(crate::cli::ToolOpts {
            plan_id: None,
            no_receipt: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("jig.schema_check is not available"));
    assert!(error.contains("sqlx_enabled = false"));
    assert!(error.contains("jig update --recopy"));
}

#[test]
fn work_goal_opens_durable_plan_and_prompt() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced and the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["Do not change public routes".into()],
            checkpoints: vec!["Capture baseline gate status".into()],
            title: Some("API goal".into()),
            notes: Some("Prefer small commits.".into()),
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert!(
        output["goal_prompt"]
            .as_str()
            .unwrap()
            .starts_with("/goal ")
    );
    assert!(output["goal_prompt"].as_str().unwrap().contains(plan_id));
    assert!(body.contains("# Goal Harness"));
    assert!(body.contains("Reduce API handler duplication"));
    assert!(body.contains("- scripts/jig work check"));
    assert!(body.contains("- [ ] Capture baseline gate status"));
    assert!(body.contains("custom: check (jig.custom_check)"));
    assert_eq!(
        output["commands"]["gates"],
        format!("scripts/jig work gates --plan-id {plan_id}")
    );
}

#[test]
fn work_goal_rejects_blank_required_fields() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let blank_validation = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["   ".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_validation.contains("--validation values cannot be empty"));

    let blank_objective = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: " \n\t ".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_objective.contains("--objective cannot be empty"));

    let blank_success = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: " \n\t ".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(blank_success.contains("--success cannot be empty"));
}

#[test]
fn work_goal_normalizes_prompt_and_defaults_missing_checkpoints() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced\nand the configured gate passes".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let body_path = output["plan"]["body_path"].as_str().unwrap();
    let body = fs::read_to_string(temp.path().join(body_path)).unwrap();
    let prompt = output["goal_prompt"].as_str().unwrap();

    assert!(prompt.contains("duplication is reduced and the configured gate passes"));
    assert!(!prompt.contains("reduced\nand"));
    assert!(body.contains("duplication is reduced\nand the configured gate passes"));
    assert!(body.contains("- [ ] Read the relevant AGENTS.md files and repo guidance."));
}

#[test]
fn work_goal_rejects_blank_checkpoints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: vec!["   ".into()],
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--checkpoint values cannot be empty"));
}

#[test]
fn work_goal_rejects_blank_constraints_when_provided() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: vec!["   ".into()],
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("--constraint values cannot be empty"));
}

#[test]
fn work_goal_truncates_generated_title_to_eighty_chars() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let objective =
        "Reduce API handler duplication while preserving every public route and fixture behavior";

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: objective.into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: None,
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title.chars().count(), 80);
    assert!(title.ends_with("..."));
}

#[test]
fn work_goal_defaults_blank_title_to_generated_title() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let output = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Goal(crate::cli::WorkGoalOpts {
            objective: "Reduce API handler duplication".into(),
            success: "duplication is reduced".into(),
            validations: vec!["scripts/jig work check".into()],
            constraints: Vec::new(),
            checkpoints: Vec::new(),
            title: Some("   ".into()),
            notes: None,
        })),
    )
    .unwrap();

    let plan_id = output["plan"]["plan_id"].as_str().unwrap();
    let plans = fs::read_to_string(temp.path().join(".agent/state/plans.jsonl")).unwrap();
    let plan_line = plans
        .lines()
        .find(|line| line.contains(plan_id))
        .expect("goal plan event should be recorded");
    let title = serde_json::from_str::<Value>(plan_line).unwrap()["title"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(title, "Reduce API handler duplication");
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
            summary: false,
        })),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(output["checks"].as_array().unwrap().len(), 1);
    assert_eq!(output["checks"][0]["tool"], "jig.custom_check");
    assert!(output["checks"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_check_rejects_unknown_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_missing".into(),
            tools: Vec::new(),
            summary: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
}

#[test]
fn work_check_rejects_closed_plan_before_running_tools() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    crate::state::plans_close(
        &ctx,
        crate::state::PlanCloseRequest {
            plan_id: "plan_1".into(),
            resolution: Some("done".into()),
        },
    )
    .unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Check(crate::cli::WorkCheckOpts {
            plan_id: "plan_1".into(),
            tools: Vec::new(),
            summary: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan is already closed: plan_1"));
    let receipts_path = temp.path().join(".agent/state/receipts.jsonl");
    let receipts = fs::read_to_string(receipts_path).unwrap_or_default();
    assert!(!receipts.contains("jig.custom_check"));
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
            summary: false,
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
            summary: false,
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
            summary: false,
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
            summary: false,
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
            summary: false,
        })),
    )
    .unwrap();

    let passed = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
            summary: false,
        })),
    )
    .unwrap();
    assert_eq!(passed["overall"], "passed");
    assert_eq!(passed["gates"][0]["status"], "passed");
    assert!(passed["gates"][0]["receipt_id"].as_str().is_some());
}

#[test]
fn work_gates_rejects_unknown_plan() {
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let error = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_missing".into(),
            summary: false,
        })),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Plan not found: plan_missing"));
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
            summary: false,
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
            summary: false,
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
            summary: false,
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
            summary: false,
        })),
    )
    .unwrap();

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: plan_id.clone(),
            summary: false,
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
            summary: false,
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
            summary: false,
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
            summary: false,
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
            summary: false,
        })),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("jig.custom_check failed with status 2"));

    let gates = dispatch(
        &ctx,
        CommandKind::Work(crate::cli::WorkCommand::Gates(crate::cli::WorkGatesOpts {
            plan_id: "plan_1".into(),
            summary: false,
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
