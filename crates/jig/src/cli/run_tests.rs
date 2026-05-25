use serde_json::json;

use crate::cli::output::format_work_review_summary;
use crate::command::{
    VaultRepoScope, VaultRuntimeOptions, VaultScopeSelection, VaultStatusRequest,
};
use crate::test_env::{CurrentDirGuard, EnvVarGuard, lock_env};

use super::*;

#[test]
fn missing_init_path_gets_actionable_hint() {
    let error = Cli::try_parse_from(["jig", "init"]).unwrap_err();
    let hint = missing_init_path_hint(&error).unwrap();

    assert!(hint.contains("jig init /path/to/new-repo"));
    assert!(hint.contains("--preset rust-react"));
    assert!(hint.contains("jig adopt ."));
    assert!(hint.contains("jig adopt . --write"));
}

#[test]
fn repo_vault_scope_is_applied_to_auto_options() {
    let mut options = VaultRuntimeOptions::default();
    let repo_root = std::path::PathBuf::from("/repo");

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", &repo_root)),
        false,
    )
    .unwrap();

    match options.scope {
        VaultScopeSelection::Repo(VaultRepoScope {
            scope_id,
            repo_name,
            repo_root: actual_root,
        }) => {
            assert_eq!(scope_id, "scope_1");
            assert_eq!(repo_name, "demo");
            assert_eq!(actual_root, repo_root);
        }
        other => panic!("expected repo scope, got {other:?}"),
    }
}

#[test]
fn repo_vault_scope_rejects_global_when_disallowed() {
    let mut options = VaultRuntimeOptions {
        home: None,
        scope: VaultScopeSelection::Global,
    };

    let error = apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("allow_global is false"));
}

#[test]
fn repo_vault_scope_allows_global_when_configured() {
    let mut options = VaultRuntimeOptions {
        home: None,
        scope: VaultScopeSelection::Global,
    };

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        true,
    )
    .unwrap();

    assert!(matches!(options.scope, VaultScopeSelection::Global));
}

#[test]
fn repo_vault_scope_leaves_auto_legacy_without_repo_scope() {
    let mut options = VaultRuntimeOptions::default();

    apply_repo_vault_scope_to_options(&mut options, None, false).unwrap();

    assert!(matches!(options.scope, VaultScopeSelection::Auto));
}

#[test]
fn repo_vault_scope_home_override_bypasses_repo_policy() {
    let home = std::path::PathBuf::from("/tmp/custom-vault");
    let mut options = VaultRuntimeOptions {
        home: Some(home.clone()),
        scope: VaultScopeSelection::Global,
    };

    apply_repo_vault_scope_to_options(
        &mut options,
        Some(VaultRuntimeOptions::repo("scope_1", "demo", "/repo")),
        false,
    )
    .unwrap();

    assert_eq!(options.home.as_deref(), Some(home.as_path()));
    assert!(matches!(options.scope, VaultScopeSelection::Global));
}

#[test]
fn vault_scope_id_validator_rejects_path_and_length_boundaries() {
    assert!(crate::command::is_valid_vault_scope_id("abc_123-XYZ"));
    assert!(!crate::command::is_valid_vault_scope_id(""));
    assert!(!crate::command::is_valid_vault_scope_id("../shared"));
    assert!(!crate::command::is_valid_vault_scope_id("scope/child"));
    assert!(!crate::command::is_valid_vault_scope_id(&"a".repeat(129)));
}

#[test]
fn explicit_vault_home_bypasses_repo_context_loading() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join(".jig.toml"),
        r#"[vault]
scope = "repo"
"#,
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());
    let explicit_home = temp.path().join("explicit-vault");
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions {
            home: Some(explicit_home.clone()),
            ..Default::default()
        },
    });

    apply_repo_vault_scope(&mut command).unwrap();

    let options = vault_options_mut(&mut command);
    assert_eq!(options.home.as_deref(), Some(explicit_home.as_path()));
    assert!(matches!(options.scope, VaultScopeSelection::Auto));
}

#[test]
fn malformed_repo_vault_config_blocks_status_without_home_override() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".agent")).unwrap();
    std::fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join(".agent/jig-contract.json"),
        json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
    let _cwd = CurrentDirGuard::set(temp.path());
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions::default(),
    });

    let error = apply_repo_vault_scope(&mut command).unwrap_err();
    let error = format!("{error:#}");

    assert!(
        error.contains("[vault].scope_id is required"),
        "unexpected error: {error}"
    );
}

#[test]
fn bootstrap_vault_capture_is_deferred_only_for_interactive_prompts() {
    assert!(!should_pre_capture_bootstrap_vault(
        true, false, false, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, true, false, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, true, false, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, false, true, true
    ));
    assert!(should_pre_capture_bootstrap_vault(
        true, false, false, false, false
    ));
    assert!(!should_pre_capture_bootstrap_vault(
        false, true, true, true, false
    ));
}

#[test]
fn no_input_bootstrap_vault_requires_env_passphrase() {
    reject_missing_no_input_vault_passphrase(true, true, true).unwrap();
    reject_missing_no_input_vault_passphrase(false, true, false).unwrap();
    reject_missing_no_input_vault_passphrase(true, false, false).unwrap();

    let error = reject_missing_no_input_vault_passphrase(true, true, false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("JIG_VAULT_PASSPHRASE is required"));
    assert!(error.contains("--no-vault"));
}

#[test]
fn pre_capture_rejects_short_new_vault_passphrase() {
    let _env = lock_env();
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "short");

    let error = runtime::capture_new_vault_passphrase()
        .unwrap_err()
        .to_string();

    assert!(error.contains("at least 12 bytes"));
}

#[test]
fn ensure_bootstrap_vault_initializes_repo_scope_and_reports_created() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".agent")).unwrap();
    std::fs::write(
        repo.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false
"#,
    )
    .unwrap();
    std::fs::write(
        repo.join(".agent/jig-contract.json"),
        json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "correct horse battery staple");
    let bootstrap = json!({ "destination": repo.display().to_string() });

    let output = ensure_bootstrap_vault(&bootstrap, true, true).unwrap();

    assert_eq!(output["requested"], true);
    assert_eq!(output["initialized"], true);
    assert_eq!(output["created"], true);
    assert_eq!(output["vault_scope"], "repo");
    assert_eq!(output["vault_scope_id"], "scope_123");
}

#[test]
fn ensure_bootstrap_vault_late_passphrase_error_mentions_written_repo_files() {
    let _env = lock_env();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".agent")).unwrap();
    std::fs::write(
        repo.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
bootstrap_command = "cargo fetch"
rust_fmt_check_command = "cargo fmt --all -- --check"
rust_clippy_command = "cargo clippy --workspace --all-targets --locked -- -D warnings"
rust_test_command = "cargo test --workspace"
rust_test_locked_command = "cargo test --workspace --locked"
web_package_manager = "bun"
frontend_apps = []

[vault]
scope = "repo"
scope_id = "scope_123"
allow_global = false
"#,
    )
    .unwrap();
    std::fs::write(
        repo.join(".agent/jig-contract.json"),
        json!({
            "contract_version": 3,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["bootstrap_command"],
            "tools": []
        })
        .to_string(),
    )
    .unwrap();
    let _vault_home = EnvVarGuard::set("JIG_VAULT_HOME", temp.path().join("vault-base"));
    let _passphrase = EnvVarGuard::set("JIG_VAULT_PASSPHRASE", "short");
    let bootstrap = json!({ "destination": repo.display().to_string() });

    let error = ensure_bootstrap_vault(&bootstrap, true, true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("repo files were written"));
    assert!(error.contains("rerun `jig vault init`"));
}

#[test]
fn vault_options_mut_reaches_nested_status_command() {
    let mut command = crate::command::VaultCommand::Status(VaultStatusRequest {
        vault: VaultRuntimeOptions::default(),
    });

    vault_options_mut(&mut command).scope = VaultScopeSelection::Global;

    match command {
        crate::command::VaultCommand::Status(request) => {
            assert!(matches!(request.vault.scope, VaultScopeSelection::Global));
        }
        other => panic!("expected status command, got {other:?}"),
    }
}

#[test]
fn missing_init_path_hint_examples_parse() {
    Cli::try_parse_from([
        "jig",
        "init",
        "/path/to/new-repo",
        "--repo-name",
        "new-repo",
        "--sqlx-enabled",
        "false",
    ])
    .unwrap();
    Cli::try_parse_from([
        "jig",
        "init",
        "/path/to/new-repo",
        "--preset",
        "rust-react",
        "--db",
        "postgres",
        "--frontends",
        "web,landing,admin",
    ])
    .unwrap();
    Cli::try_parse_from(["jig", "adopt", "."]).unwrap();
    Cli::try_parse_from(["jig", "adopt", ".", "--write"]).unwrap();
}

#[test]
fn unrelated_parse_errors_do_not_get_missing_init_path_hint() {
    let missing_proxy_args = Cli::try_parse_from(["jig", "proxy", "run"]).unwrap_err();
    assert_eq!(
        missing_proxy_args.kind(),
        clap::error::ErrorKind::MissingRequiredArgument
    );
    assert!(missing_init_path_hint(&missing_proxy_args).is_none());

    let invalid_subcommand = Cli::try_parse_from(["jig", "not-a-command"]).unwrap_err();
    assert!(missing_init_path_hint(&invalid_subcommand).is_none());
}

#[test]
fn vault_run_summary_reports_status_and_redacted_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 2,
            "exit_signal": null,
            "stdout": "redacted stdout",
            "stderr": "redacted stderr"
        }
    }));

    assert!(summary.contains("Vault run: exit 2"));
    assert!(summary.contains("stdout: redacted stdout"));
    assert!(summary.contains("stderr: redacted stderr"));
}

#[test]
fn adopt_human_summary_includes_notes() {
    let summary = format_adopt_human_summary(&json!({
        "render_mode": "preview",
        "destination": "/tmp/repo",
        "render_report": {
            "files_created": [],
            "files_modified": [],
            "files_removed": [],
            "conflicts": []
        },
        "vault": {
            "requested": false
        },
        "adoption_review": [],
        "notes": [
            "Existing .jig.toml had no [vault] block, so Jig added a new repo-scoped vault scope."
        ],
        "detection_report": {
            "warnings": []
        },
        "next_steps": []
    }));

    assert!(summary.contains("notes:"));
    assert!(summary.contains("repo-scoped vault scope"));
}

#[test]
fn vault_run_summary_calls_out_truncated_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 1,
            "exit_signal": null,
            "stdout": "x ".repeat(260),
            "stderr": ""
        }
    }));

    assert!(summary.contains("stdout: "));
    assert!(summary.contains("Output truncated; rerun without --summary for full JSON."));
}

#[test]
fn vault_run_summary_preserves_short_multiline_output() {
    let summary = format_vault_run_summary(&json!({
        "result": {
            "exit_status": 1,
            "exit_signal": null,
            "stdout": "",
            "stderr": "first line\nsecond line"
        }
    }));

    assert!(summary.contains("stderr: first line\nsecond line"));
}

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
fn work_check_summary_is_actionable() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(summary.contains("Plan: plan_1"));
    assert!(summary.contains("Batch receipt: receipt_batch"));
    assert!(summary.contains("jig.test: exit 0, receipt receipt_test"));
    assert!(summary.contains("work gates --plan-id plan_1 --summary"));
}

#[test]
fn work_check_summary_reports_failed_check_status() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: failed"));
    assert!(summary.contains("jig.test: exit 101"));
    assert!(summary.contains("inspect failing receipts"));
    assert!(!summary.contains("work gates --plan-id plan_1 --summary"));
}

#[test]
fn work_check_summary_reports_failed_check_output() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "",
                "stderr": "test failure details\n"
            }
        }]
    }));

    assert!(summary.contains("jig.test: exit 101"));
    assert!(summary.contains("output: test failure details"));
}

#[test]
fn work_check_summary_surfaces_skipped_harness_defaults() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "No Cargo.toml found; skipping cargo test.\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: passed (all skipped)"));
    assert!(summary.contains("output: No Cargo.toml found; skipping cargo test."));
    assert!(summary.contains("all configured Cargo checks skipped"));
    assert!(summary.contains("set explicit commands"));
}

#[test]
fn work_check_summary_reports_some_skipped_harness_defaults() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [
            {
                "tool": "jig.fmt",
                "receipt_id": "receipt_fmt",
                "result": {
                    "exit_status": 0,
                    "stdout": "No Cargo.toml found; skipping cargo fmt.\n",
                    "stderr": ""
                }
            },
            {
                "tool": "jig.contract_check",
                "receipt_id": "receipt_contract",
                "result": {
                    "exit_status": 0,
                    "stdout": "",
                    "stderr": ""
                }
            }
        ]
    }));

    assert!(summary.contains("Work check: passed (some skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

#[test]
fn work_check_summary_ignores_unrelated_skipping_output() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "skipping optional project step\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("jig.test: exit 0, receipt receipt_test"));
    assert!(!summary.contains("output:"));
}

#[test]
fn work_check_summary_does_not_treat_stderr_skip_text_as_harness_skip() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 0,
                "stdout": "",
                "stderr": "No Cargo.toml found; skipping cargo test.\n"
            }
        }]
    }));

    assert!(summary.contains("Work check: passed"));
    assert!(!summary.contains("passed (all skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

#[test]
fn work_check_summary_does_not_count_failed_prefix_output_as_skipped() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "exit_status": 101,
                "stdout": "No Cargo.toml found; skipping cargo test.\n",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: failed"));
    assert!(!summary.contains("passed (all skipped)"));
    assert!(!summary.contains("all configured Cargo checks skipped"));
}

#[test]
fn work_check_summary_reports_empty_checks() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": []
    }));

    assert!(summary.contains("Work check: no checks configured"));
    assert!(summary.contains("Checks: 0"));
    assert!(summary.contains("configure work checks"));
    assert!(summary.contains("--tool <tool>"));
}

#[test]
fn work_check_summary_reports_unknown_check_status() {
    let summary = format_work_check_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "receipt_id": "receipt_batch",
        "checks": [{
            "tool": "jig.test",
            "receipt_id": "receipt_test",
            "result": {
                "stdout": "",
                "stderr": ""
            }
        }]
    }));

    assert!(summary.contains("Work check: unknown"));
    assert!(summary.contains("jig.test: exit ?"));
    assert!(summary.contains("unknown exit status"));
    assert!(!summary.contains("inspect failing receipts"));
}

#[test]
fn work_gates_summary_reports_blockers() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "missing",
            "freshness": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: blocked"));
    assert!(summary.contains("tests: missing, freshness missing, required (jig.test)"));
    assert!(summary.contains("Blocked: missing (tests)"));
    assert!(!summary.contains("failed ()"));
    assert!(summary.contains("work check --plan-id plan_1 --summary"));
}

#[test]
fn work_gates_summary_reports_unsupported_blockers() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": ["schema"]
    }));

    assert!(summary.contains("Blocked: unsupported (schema)"));
}

#[test]
fn work_gates_summary_reports_combined_blockers_in_stable_order() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "blocked",
        "gates": [],
        "missing_required": ["tests"],
        "failed_required": ["lint"],
        "stale_required": ["schema"],
        "unknown_required": ["docs"],
        "unsupported_required": ["deploy"]
    }));

    assert!(summary.contains(
        "Blocked: missing (tests); failed (lint); stale (schema); unknown (docs); unsupported (deploy)"
    ));
}

#[test]
fn work_gates_summary_handles_uncategorized_blocked_status() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "overall": "unknown",
        "gates": [],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: unknown"));
    assert!(summary.contains("Status: unknown; no categorized blockers reported"));
}

#[test]
fn work_gates_summary_reports_finish_command_when_passed() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "passed",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "passed",
            "freshness": "fresh"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work gates: passed"));
    assert!(summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_gates_summary_does_not_offer_finish_for_closed_plan() {
    let summary = format_work_gates_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "passed",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "passed",
            "freshness": "fresh"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: none; plan is closed"));
    assert!(!summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_gates_summary_does_not_offer_check_for_closed_blocked_plan() {
    let summary = format_work_gates_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "blocked",
        "gates": [{
            "id": "tests",
            "kind": "check",
            "required": true,
            "tool": "jig.test",
            "status": "missing",
            "freshness": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: start a new work plan for follow-up changes"));
    assert!(!summary.contains("work check --plan-id plan_1"));
}

#[test]
fn work_evidence_summary_reports_latest_gate_freshness_and_paths() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "open",
        "overall": "blocked",
        "latest_passing_gates": [{
            "tool": "jig.test",
            "gate_id": "tests",
            "receipt_id": "receipt_tool",
            "freshness_receipt_id": "receipt_batch",
            "matches_current_worktree": false,
            "freshness": "stale",
            "freshness_reason": "receipt was recorded for a different worktree fingerprint",
            "diff_summary": "2 files, +4 -1",
            "changed_paths": ["src/lib.rs", "Cargo.toml"]
        }],
        "gates": [{
            "id": "tests",
            "status": "stale",
            "freshness_reason": "receipt was recorded for a different worktree fingerprint"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": ["tests"],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Work evidence: blocked"));
    assert!(summary.contains("Latest gate evidence per gate:"));
    assert!(
        summary.contains(
            "jig.test: tests, receipt receipt_batch, matches current worktree no (stale)"
        )
    );
    assert!(summary.contains("receipt was recorded for a different worktree fingerprint"));
    assert!(summary.contains("changed paths covered: src/lib.rs, Cargo.toml"));
    assert!(summary.contains("Next step: scripts/jig work check --plan-id plan_1 --summary"));
}

#[test]
fn work_evidence_summary_reports_closed_plan_as_done() {
    let summary = format_work_evidence_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "passed",
        "latest_passing_gates": [],
        "gates": [{
            "id": "tests",
            "status": "passed"
        }],
        "missing_required": [],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Unresolved gates: none"));
    assert!(summary.contains("Next step: none; plan is closed"));
    assert!(!summary.contains("work finish --plan-id plan_1"));
}

#[test]
fn work_evidence_summary_does_not_offer_check_for_closed_blocked_plan() {
    let summary = format_work_evidence_summary(&json!({
        "ok": false,
        "plan_id": "plan_1",
        "plan_state": "closed",
        "overall": "blocked",
        "latest_passing_gates": [],
        "gates": [{
            "id": "tests",
            "status": "missing"
        }],
        "missing_required": ["tests"],
        "failed_required": [],
        "stale_required": [],
        "unknown_required": [],
        "unsupported_required": []
    }));

    assert!(summary.contains("Plan: plan_1 (closed)"));
    assert!(summary.contains("Next step: start a new work plan for follow-up changes"));
    assert!(!summary.contains("work check --plan-id plan_1"));
}

#[test]
fn work_review_summary_reports_truncated_counts() {
    let summary = format_work_review_summary(&json!({
        "ok": true,
        "plan_id": "plan_1",
        "status": "failed",
        "reviews": [{
            "gate_id": "rust-error-handling",
            "status": "failed",
            "skill": "jig-rust:rust-error-handling-review",
            "finding_count": 105,
            "actionable_count": 105,
            "retained_finding_count": 100,
            "retained_actionable_count": 100,
            "findings_truncated": true,
            "actionable_findings_truncated": true
        }]
    }));

    assert!(summary.contains("105/105 actionable, showing 100/100"));
    assert!(summary.contains("Next step: scripts/jig work refine --plan-id plan_1 --summary"));
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
    assert!(summary.contains("output: Diff in src/lib.rs:\n- old\n+ new"));
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
