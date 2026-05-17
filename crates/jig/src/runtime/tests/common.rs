use super::*;

pub(super) fn write_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

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
            "jig_version": "0.2.0-beta.1",
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
    write_open_plan(root);
}

pub(super) fn write_command_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"
rust_migration_dir = "migrations"
rust_sqlx_metadata_dir = ".sqlx"
schema_dump_command = "printf 'schema dump\n'"
rust_test_command = "printf 'command tool ran\n'"
contract_check_command = "printf 'contract ok\n'"

[[work.gates]]
id = "custom"
kind = "check"
tool = "jig.custom_check"
"#,
    )
    .unwrap();
    fs::write(
        root.join(".agent/jig-contract.json"),
        serde_json::to_string_pretty(&json!({
            "contract_version": 2,
            "tool_namespace": "jig",
            "jig_version": "0.2.0-beta.1",
            "required_commands": ["rust_test_command"],
            "tools": [
                {
                    "name": "jig.custom_check",
                    "kind": "command",
                    "description": "Run configured custom check.",
                    "command": "rust_test_command"
                }
            ],
        }))
        .unwrap(),
    )
    .unwrap();
    write_open_plan(root);
}

pub(super) fn write_mutating_check_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

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
            "jig_version": "0.2.0-beta.1",
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
    write_open_plan(root);
}

pub(super) fn write_failing_check_fixture_repo(root: &Path) {
    fs::create_dir_all(root.join(".agent")).unwrap();
    fs::write(
        root.join(".jig.toml"),
        r#"_src_path = "/tmp/template"
_commit = "abc123"
repo_name = "demo"
default_branch = "main"
jig_version = "0.2.0-beta.1"

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
            "jig_version": "0.2.0-beta.1",
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
    write_open_plan(root);
}

fn write_open_plan(root: &Path) {
    let ctx = RepoContext::load_from(root).unwrap();
    crate::state::seed_open_plan_for_test(&ctx, "plan_1", "Test plan", "# Test plan\n").unwrap();
}

pub(super) fn open_test_plan(ctx: &RepoContext) -> String {
    // Most runtime fixtures seed plan_1 because work-check tests exercise that
    // stable id directly. Reuse it while it remains open; otherwise fall back to
    // opening a fresh plan for tests that deliberately closed the seeded one.
    if crate::state::ensure_plan_is_open(ctx, "plan_1").is_ok() {
        return "plan_1".into();
    }

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

pub(super) struct TestReceipt<'a> {
    pub(super) tool_name: &'a str,
    pub(super) args: Value,
    pub(super) plan_id: &'a str,
    pub(super) started_at_ms: u64,
    pub(super) ended_at_ms: u64,
    pub(super) worktree_fingerprint: Option<String>,
}

pub(super) fn record_test_receipt(ctx: &RepoContext, receipt: TestReceipt<'_>) -> String {
    record_receipt(
        ctx,
        ReceiptInput {
            tool_name: receipt.tool_name,
            args: receipt.args,
            invoked_make_target: None,
            invoked_command_key: None,
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

pub(super) fn init_git_repo(root: &Path) {
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

pub(super) fn write_codex_stub(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
