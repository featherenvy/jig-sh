use serde_json::json;

use super::*;

#[test]
fn agent_doctor_summary_calls_out_source_mismatch() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": false,
        "codex": {
            "required": true,
            "available": true
        },
        "marketplaces": [{
            "id": "jig-skills",
            "source": "bpcakes/jig-skills",
            "configured_source": "https://github.com/example/jig-skills.git",
            "registered": false
        }],
        "next_steps": [
            "Run `scripts/jig agent bootstrap` to register marketplace jig-skills."
        ]
    }));

    assert!(summary.contains("Agent tooling: needs setup"));
    assert!(summary.contains("repo config expects bpcakes/jig-skills"));
    assert!(summary.contains("Codex has https://github.com/example/jig-skills.git"));
    assert!(summary.contains("Next steps:"));
}

#[test]
fn agent_doctor_summary_handles_optional_codex_requirement() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": true,
        "codex": {
            "required": false,
            "available": null
        },
        "marketplaces": [],
        "next_steps": []
    }));

    assert!(summary.contains("Agent tooling: ready"));
    assert!(summary.contains("Codex: not required (probe skipped)"));
    assert!(summary.contains("Marketplaces: none configured"));
    // Regression guard for the previously duplicated requirement/probe label.
    assert!(!summary.contains("not required (not required)"));
    // When Codex is not required, the summary should explain the skipped
    // probe instead of exposing the underlying null availability field.
    assert!(!summary.contains("unknown"));
}

#[test]
fn agent_doctor_summary_handles_ready_marketplace() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": true,
        "codex": {
            "required": true,
            "available": true
        },
        "marketplaces": [{
            "id": "jig-skills",
            "source": "bpcakes/jig-skills",
            "configured_source": "https://github.com/bpcakes/jig-skills.git",
            "registered": true
        }],
        "next_steps": []
    }));

    assert!(summary.contains("Agent tooling: ready"));
    assert!(summary.contains("Codex: required (available)"));
    assert!(summary.contains("jig-skills: registered"));
    assert!(summary.contains("Next steps: none"));
}

#[test]
fn agent_doctor_summary_handles_unknown_required_codex_availability() {
    let summary = format_agent_doctor_summary(&json!({
        "ok": false,
        "codex": {
            "required": true,
            "available": null
        },
        "marketplaces": [],
        "next_steps": []
    }));

    assert!(summary.contains("Codex: required (unknown)"));
}

#[test]
fn work_status_summary_stays_compact() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 1,
            "receipts": 12,
            "failed_receipts": 2,
            "decisions": 3
        },
        "open_plans": [{
            "plan_id": "plan_1",
            "title": "Improve UX"
        }],
        "recent_receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.test",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.clippy",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_3",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_4",
                "tool_name": "jig.contract_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_5",
                "tool_name": "jig.bootstrap",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_6",
                "tool_name": "jig.extra",
                "exit_status": 0,
                "diff_summary": "no changes"
            }
        ]
    }));

    assert!(summary.contains("Work status:"));
    assert!(summary.contains("Plans: 1 open"));
    assert!(summary.contains("Receipts: 12 total, 2 failed"));
    assert!(summary.contains("Decisions: 3"));
    assert!(summary.contains("Repo: demo (main)"));
    assert!(summary.contains("Current session: none"));
    assert!(summary.contains("plan_1: Improve UX"));
    assert!(summary.contains("jig.test"));
    assert!(summary.contains("and 1 more recent receipt"));
}

#[test]
fn work_start_plan_id_output_is_shell_friendly() {
    let plan_id = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": {
            "plan_id": "plan_123"
        }
    }))
    .unwrap();

    assert_eq!(plan_id, "plan_123");
}

#[test]
fn work_start_plan_id_output_requires_plan_id() {
    let error = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": {}
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("plan.plan_id"));
}

#[test]
fn work_start_plan_id_output_requires_plan_object() {
    let error = format_work_start_plan_id(&json!({
        "ok": true
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("include plan"));
}

#[test]
fn work_start_plan_id_output_requires_plan_to_be_object() {
    let error = format_work_start_plan_id(&json!({
        "ok": true,
        "plan": null
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("plan was not an object"));
}

#[test]
fn work_status_summary_omits_truncation_hint_at_receipt_limit() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 0,
            "receipts": 5,
            "failed_receipts": 0,
            "decisions": 0
        },
        "open_plans": [],
        "recent_receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.test",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.clippy",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_3",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_4",
                "tool_name": "jig.contract_check",
                "exit_status": 0,
                "diff_summary": "no changes"
            },
            {
                "id": "receipt_5",
                "tool_name": "jig.bootstrap",
                "exit_status": 0,
                "diff_summary": "no changes"
            }
        ]
    }));

    assert!(summary.contains("receipt_5"));
    assert!(!summary.contains("omit --summary"));
}

#[test]
fn work_status_summary_handles_empty_state() {
    let summary = format_work_status_summary(&json!({
        "repo": {
            "name": "demo",
            "default_branch": "main"
        },
        "current_session_id": null,
        "counts": {
            "open_plans": 0,
            "receipts": 0,
            "failed_receipts": 0,
            "decisions": 0
        },
        "open_plans": [],
        "recent_receipts": []
    }));

    assert!(summary.contains("Plans: 0 open"));
    assert!(summary.contains("Receipts: 0 total, 0 failed"));
    assert!(summary.contains("Decisions: 0"));
    assert!(summary.contains("Current session: none"));
    assert!(summary.contains("Open plans: none"));
    assert!(summary.contains("Recent receipts: none"));
}

#[test]
fn work_receipts_summary_is_compact() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.fmt_check",
            "exit_status": 1,
            "diff_summary": "2 files, +4 -1",
            "plan_id": "plan_1",
            "session_id": null,
            "stdout_preview": "Diff in src/lib.rs:\n- old\n+ new",
            "stderr_preview": ""
        }]
    }));

    assert!(summary.contains("Work receipts:"));
    assert!(summary.contains("Showing: 1"));
    assert!(summary.contains("jig.fmt_check (receipt_1): exit 1, 2 files, +4 -1"));
    assert!(summary.contains("plan: plan_1; session: none"));
    assert!(summary.contains("output: Diff in src/lib.rs: - old + new"));
}

#[test]
fn work_receipts_summary_prefers_stderr_and_truncates() {
    let long_stderr = "error ".repeat(80);
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.clippy",
            "exit_status": 101,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": "session_1",
            "stdout_preview": "stdout should not win",
            "stderr_preview": long_stderr
        }]
    }));

    assert!(summary.contains("plan: none; session: session_1"));
    assert!(summary.contains("output: error error"));
    assert!(summary.contains("..."));
    assert!(!summary.contains("stdout should not win"));
}

#[test]
fn work_receipts_summary_uses_stdout_when_stderr_is_empty() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null,
            "stdout_preview": "test output",
            "stderr_preview": ""
        }]
    }));

    assert!(summary.contains("output: test output"));
}

#[test]
fn work_receipts_summary_lists_multiple_receipts() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [
            {
                "id": "receipt_1",
                "tool_name": "jig.fmt_check",
                "exit_status": 0,
                "diff_summary": "no changes",
                "plan_id": "plan_1",
                "session_id": null,
                "stdout_preview": "",
                "stderr_preview": ""
            },
            {
                "id": "receipt_2",
                "tool_name": "jig.test",
                "exit_status": 101,
                "diff_summary": "1 file, +1 -0",
                "plan_id": "plan_1",
                "session_id": "session_1",
                "stdout_preview": "tests failed",
                "stderr_preview": ""
            }
        ]
    }));

    assert!(summary.contains("Showing: 2"));
    assert!(summary.contains("jig.fmt_check (receipt_1): exit 0, no changes"));
    assert!(summary.contains("jig.test (receipt_2): exit 101, 1 file, +1 -0"));
    assert!(summary.contains("output: tests failed"));
    assert!(!summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_handles_empty_results() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": []
    }));

    assert!(summary.contains("Showing: 0"));
    assert!(summary.contains("No receipts matched"));
}

#[test]
fn work_receipts_summary_omits_output_line_without_preview() {
    let summary = format_work_receipts_summary(&json!({
        "ok": true,
        "receipts": [{
            "id": "receipt_1",
            "tool_name": "jig.test",
            "exit_status": 0,
            "diff_summary": "no changes",
            "plan_id": null,
            "session_id": null
        }]
    }));

    assert!(summary.contains("jig.test (receipt_1): exit 0, no changes"));
    assert!(summary.contains("plan: none; session: none"));
    assert!(!summary.contains("output:"));
}
