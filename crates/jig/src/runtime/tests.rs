use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use crate::test_env::{EnvVarGuard, lock_env};

use super::*;

mod mcp;

fn write_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
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
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[work.gates]]
id = "first"
kind = "check"
tool = "jig.first_check"

[[work.gates]]
id = "mutating"
kind = "check"
tool = "jig.mutating_check"
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
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
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

fn write_codex_stub(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
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
fn agent_doctor_reports_configured_codex_marketplace() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("bpcakes/jig-skills")).unwrap();
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"

[plugins."jig-rust@jig-skills"]
enabled = true

[plugins."jig-swift@jig-skills"]
enabled = true

[plugins."jig-typescript@jig-skills"]
enabled = true

[plugins."jig-exec-plans@jig-skills"]
enabled = true
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["codex"]["available"], true);
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["source_matches"], true);
    assert_eq!(output["marketplaces"][0]["plugins_ready"], true);
    assert_eq!(output["marketplaces"][0]["plugins"][0]["enabled"], true);
}

#[test]
fn agent_doctor_accepts_registered_marketplace_without_plugin_entries() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["readiness"]["ok_requires_plugins_enabled"], false);
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["plugins_ready"], false);
    assert_eq!(output["marketplaces"][0]["plugins"][0]["enabled"], false);
}

#[test]
fn agent_doctor_reports_source_mismatch_for_registered_marketplace_id() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/someone-else/jig-skills.git"

[plugins."jig-rust@jig-skills"]
enabled = true

[plugins."jig-swift@jig-skills"]
enabled = true

[plugins."jig-typescript@jig-skills"]
enabled = true

[plugins."jig-exec-plans@jig-skills"]
enabled = true
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["marketplaces"][0]["registered"], false);
    assert_eq!(output["marketplaces"][0]["source_matches"], false);
    assert_eq!(
        output["marketplaces"][0]["configured_source"],
        "https://github.com/someone-else/jig-skills.git"
    );
}

#[test]
fn agent_doctor_reports_unsupported_codex_when_marketplace_required() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        r#"[marketplaces.jig-skills]
source_type = "git"
source = "https://github.com/bpcakes/jig-skills.git"
"#,
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(&codex_path, "#!/bin/sh\nexit 2\n");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], false, "{output:#}");
    assert_eq!(output["codex"]["required"], true);
    assert_eq!(output["codex"]["available"], false);
    assert_eq!(output["codex"]["probe_skipped"], false);
    assert_eq!(output["marketplaces"][0]["registered"], true);
}

#[test]
fn agent_doctor_matches_relative_config_to_absolute_codex_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    let repo_root = temp.path().join("repo");
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&repo_root).unwrap();
    fs::create_dir_all(&skills_root).unwrap();
    write_fixture_repo(&repo_root);
    fs::write(
        repo_root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "../jig-skills"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let codex_home = temp.path().join("codex-home");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"[marketplaces.local-skills]
source_type = "path"
source = "{}"
"#,
            skills_root.display()
        ),
    )
    .unwrap();
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(&repo_root).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["marketplaces"][0]["registered"], true);
    assert_eq!(output["marketplaces"][0]["source_matches"], true);
}

#[test]
fn agent_doctor_accepts_empty_marketplace_config_without_codex() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[agent_tooling.codex]
marketplaces = []

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let missing_codex = temp.path().join("missing-codex");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &missing_codex);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(output["ok"], true, "{output:#}");
    assert_eq!(output["codex"]["probe_skipped"], true);
    assert_eq!(output["codex"]["available"], serde_json::Value::Null);
    assert_eq!(output["codex"]["config_read"], false);
    assert!(output["marketplaces"].as_array().unwrap().is_empty());
}

#[test]
fn agent_bootstrap_invokes_codex_marketplace_add() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&skills_root).unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("./jig-skills".into()),
            },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_bootstrap_then_doctor_passes_with_marketplace_registration() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::create_dir_all(temp.path().join("bpcakes/jig-skills")).unwrap();
    let codex_home = temp.path().join("codex-home");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nif [ \"$1 $2 $3 $4\" = \"plugin marketplace add --help\" ]; then exit 0; fi\nif [ \"$1 $2 $3\" = \"plugin marketplace add\" ]; then\n  mkdir -p \"$CODEX_HOME\"\n  cat > \"$CODEX_HOME/config.toml\" <<'EOF'\n[marketplaces.jig-skills]\nsource_type = \"git\"\nsource = \"https://github.com/bpcakes/jig-skills.git\"\nEOF\n  exit 0\nfi\nexit 2\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _codex_home = EnvVarGuard::set("CODEX_HOME", &codex_home);
    let ctx = RepoContext::load_from(temp.path()).unwrap();

    let bootstrap_output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();
    let doctor_output =
        dispatch(&ctx, CommandKind::Agent(crate::cli::AgentCommand::Doctor)).unwrap();

    assert_eq!(bootstrap_output["ok"], true);
    assert_eq!(bootstrap_output["marketplace_source"], "bpcakes/jig-skills");
    assert_eq!(doctor_output["ok"], true, "{doctor_output:#}");
    assert_eq!(
        doctor_output["readiness"]["ok_requires_plugins_enabled"],
        false
    );
    assert_eq!(doctor_output["marketplaces"][0]["registered"], true);
    assert_eq!(doctor_output["marketplaces"][0]["plugins_ready"], false);
    assert_eq!(
        doctor_output["marketplaces"][0]["plugins"][0]["enabled"],
        false
    );
}

#[test]
fn agent_bootstrap_uses_single_configured_marketplace_source() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("jig-skills");
    fs::create_dir_all(&skills_root).unwrap();
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[agent_tooling.codex.marketplaces]]
id = "local-skills"
source = "./jig-skills"
plugins = ["local-rust@local-skills"]

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_bootstrap_uses_marketplace_env_override() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let skills_root = temp.path().join("env-skills");
    fs::create_dir_all(&skills_root).unwrap();
    let log_path = temp.path().join("codex.log");
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nexit 0\n",
            log_path.display()
        ),
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let _marketplace = EnvVarGuard::set("JIG_SKILLS_MARKETPLACE", "./env-skills");
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let output = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap();

    assert_eq!(output["ok"], true);
    assert_eq!(
        output["marketplace_source"],
        skills_root.canonicalize().unwrap().display().to_string()
    );
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains(&format!(
        "plugin marketplace add {}",
        skills_root.canonicalize().unwrap().display()
    )));
}

#[test]
fn agent_bootstrap_rejects_missing_relative_marketplace_path() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts {
                marketplace: Some("./missing-skills".into()),
            },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Configured Codex marketplace path ./missing-skills does not exist"));
    assert!(error.contains(&temp.path().display().to_string()));
}

#[test]
fn agent_bootstrap_rejects_ambiguous_configured_marketplaces() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    fs::write(
        temp.path().join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.1.0"

[[agent_tooling.codex.marketplaces]]
id = "first-skills"
source = "../first-skills"

[[agent_tooling.codex.marketplaces]]
id = "second-skills"
source = "../second-skills"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();

    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Multiple Codex marketplaces are configured"));
    assert!(error.contains("first-skills=../first-skills"));
    assert!(error.contains("pass --marketplace <source>"));
}

#[test]
fn agent_bootstrap_fails_when_codex_marketplace_add_fails() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let codex_path = temp.path().join("codex-stub.sh");
    write_codex_stub(
        &codex_path,
        "#!/bin/sh\nprintf 'bad source\\n' >&2\nexit 9\n",
    );

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &codex_path);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("plugin marketplace add bpcakes/jig-skills failed"));
    assert!(error.contains("exit status 9"));
    assert!(error.contains("bad source"));
}

#[test]
fn agent_bootstrap_fails_when_codex_cannot_be_started() {
    let _guard = lock_env();
    let temp = tempdir().unwrap();
    write_fixture_repo(temp.path());
    let missing_codex = temp.path().join("missing-codex");

    let _codex_bin = EnvVarGuard::set("JIG_CODEX_BIN", &missing_codex);
    let ctx = RepoContext::load_from(temp.path()).unwrap();
    let error = dispatch(
        &ctx,
        CommandKind::Agent(crate::cli::AgentCommand::Bootstrap(
            crate::cli::AgentBootstrapOpts { marketplace: None },
        )),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Failed to run"));
    assert!(error.contains("plugin marketplace add bpcakes/jig-skills"));
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
