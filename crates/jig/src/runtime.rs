use std::borrow::Cow;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::CommandKind;
use crate::context::{ManifestTool, RepoContext};
use crate::process::require_success;
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::{self, JsonObject, MemoryTool, args, string_arg, tool};

mod requests;
mod work;

pub(crate) fn dispatch(ctx: &RepoContext, command: CommandKind) -> Result<Value> {
    match command {
        CommandKind::FmtCheck(opts) => {
            execute_manifest_make_tool(ctx, tool::FMT_CHECK, json!({}), opts.plan_id)
        }
        CommandKind::Clippy(opts) => {
            execute_manifest_make_tool(ctx, tool::CLIPPY, json!({}), opts.plan_id)
        }
        CommandKind::Test(opts) => {
            execute_manifest_make_tool(ctx, tool::TEST, json!({}), opts.plan_id)
        }
        CommandKind::TestLocked(opts) => {
            execute_manifest_make_tool(ctx, tool::TEST_LOCKED, json!({}), opts.plan_id)
        }
        CommandKind::SqlxCheck(opts) => {
            execute_manifest_make_tool(ctx, tool::SQLX_CHECK, json!({}), opts.plan_id)
        }
        CommandKind::SchemaCheck(opts) => {
            execute_manifest_make_tool(ctx, tool::SCHEMA_CHECK, json!({}), opts.plan_id)
        }
        CommandKind::SchemaDump(opts) => {
            execute_manifest_make_tool(ctx, tool::SCHEMA_DUMP, json!({}), opts.plan_id)
        }
        CommandKind::MigrationAdd(opts) => execute_manifest_make_tool(
            ctx,
            tool::MIGRATION_ADD,
            json!({ args::NAME: opts.name }),
            opts.tool.plan_id,
        )
        .map(|value| {
            let name = value["args"][args::NAME].clone();
            json!({
                "ok": true,
                "tool": tool::MIGRATION_ADD,
                args::NAME: name,
                "result": value["result"],
                "receipt_id": value["receipt_id"],
            })
        }),
        CommandKind::ContractCheck(opts) => {
            execute_manifest_make_tool(ctx, tool::CONTRACT_CHECK, json!({}), opts.plan_id)
        }
        CommandKind::RunTarget(opts) => execute_manifest_make_tool(
            ctx,
            tool::RUN_TARGET,
            json!({ args::NAME: opts.name }),
            opts.tool.plan_id,
        ),
        CommandKind::Work(command) => work::dispatch(ctx, command),
        CommandKind::Init(_) | CommandKind::Adopt(_) | CommandKind::Update(_) => unreachable!(),
        CommandKind::Mcp => unreachable!(),
    }
}

pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    let args_obj = args.as_object().cloned().unwrap_or_default();

    if let Some(tool) = ctx.tool_spec(name)
        && tool_defs::is_make_tool(tool)
    {
        return call_manifest_make_tool(ctx, tool, &args_obj);
    }

    match MemoryTool::from_name(name) {
        Some(MemoryTool::Start) => work::start_from_args(ctx, &args_obj),
        Some(MemoryTool::Append) => work::append_from_args(ctx, &args_obj),
        Some(MemoryTool::Check) => work::check_from_args(ctx, &args_obj),
        Some(MemoryTool::Gates) => work::gates_from_args(ctx, &args_obj),
        Some(MemoryTool::Decide) => work::decide_from_args(ctx, &args_obj),
        Some(MemoryTool::Receipts) => work::receipts_from_args(ctx, &args_obj),
        Some(MemoryTool::Status) => crate::state::state_summary(ctx),
        Some(MemoryTool::Finish) => work::finish_from_args(ctx, &args_obj),
        None => bail!("Unsupported tool: {name}"),
    }
}

fn call_manifest_make_tool(
    ctx: &RepoContext,
    tool: &ManifestTool,
    args_obj: &JsonObject,
) -> Result<Value> {
    let plan_id = string_arg(args_obj, args::PLAN_ID);
    let args = tool_defs::make_tool_args(tool, args_obj)?;

    execute_manifest_make_tool(ctx, &tool.name, args, plan_id)
}

fn execute_manifest_make_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
) -> Result<Value> {
    execute_manifest_make_tool_with_options(ctx, tool_name, args, plan_id, true)
}

fn execute_manifest_make_tool_without_worktree_fingerprint(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
) -> Result<Value> {
    execute_manifest_make_tool_with_options(ctx, tool_name, args, plan_id, false)
}

fn execute_manifest_make_tool_with_options(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let tool = ctx
        .tool_spec(tool_name)
        .ok_or_else(|| anyhow!("Tool is not declared in .agent/jig-contract.json: {tool_name}"))?;
    if !tool_defs::is_make_tool(tool) {
        bail!("Tool is not a make-backed tool: {tool_name}");
    }

    let target = match tool.target.as_deref() {
        Some(target) => Cow::Borrowed(target),
        None => args
            .get(args::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{tool_name} requires a name argument"))?
            .to_string()
            .into(),
    };

    execute_make_tool(
        ctx,
        &tool.name,
        target.as_ref(),
        args,
        plan_id,
        collect_worktree_fingerprint,
    )
}

fn execute_make_tool(
    ctx: &RepoContext,
    tool_name: &str,
    target: &str,
    args: Value,
    plan_id: Option<String>,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let started = now_ms();
    let output = run_make(ctx, target, &args)?;
    let ended = now_ms();
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_make_target: Some(target.to_string()),
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status,
            stdout: &stdout,
            stderr: &stderr,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
    )?;

    require_success(&output, |_| {
        format!(
            "{tool_name} failed with status {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        )
    })?;

    Ok(json!({
        "ok": true,
        "tool": tool_name,
        "target": target,
        "args": args,
        "result": {
            "exit_status": exit_status,
            "stdout": stdout,
            "stderr": stderr,
        },
        "receipt_id": receipt_id,
    }))
}

fn run_make(ctx: &RepoContext, target: &str, args: &Value) -> Result<Output> {
    let mut command = Command::new("make");
    command.current_dir(ctx.root()).arg(target);

    if target == tool_defs::cli_command::MIGRATION_ADD {
        let name = args
            .get(args::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow!(
                    "{} requires a name argument",
                    tool_defs::cli_command::MIGRATION_ADD
                )
            })?;
        command.arg(format!("NAME={name}"));
    }

    command
        .output()
        .with_context(|| format!("Failed to run make {target}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
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

    fn append_receipt(root: &Path, receipt: Value) {
        let path = root.join(".agent/state/receipts.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "{receipt}").unwrap();
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

        append_receipt(
            temp.path(),
            json!({
                "id": "receipt_old_batch",
                "session_id": null,
                "plan_id": "plan_1",
                "tool_name": "jig.work_check",
                "args": { "plan_id": "plan_1", "tools": ["jig.custom_check"] },
                "invoked_make_target": null,
                "started_at_ms": 100,
                "ended_at_ms": 200,
                "exit_status": 0,
                "stdout_preview": "",
                "stderr_preview": "",
                "changed_paths": [],
                "diff_stat": { "files": 0, "insertions": 0, "deletions": 0 },
                "git_status_error": null,
                "git_diff_stat_error": null,
                "worktree_fingerprint": "stale-fingerprint",
                "worktree_fingerprint_error": null
            }),
        );
        append_receipt(
            temp.path(),
            json!({
                "id": "receipt_direct",
                "session_id": null,
                "plan_id": "plan_1",
                "tool_name": "jig.custom_check",
                "args": {},
                "invoked_make_target": "custom-check",
                "started_at_ms": 200,
                "ended_at_ms": 200,
                "exit_status": 0,
                "stdout_preview": "",
                "stderr_preview": "",
                "changed_paths": [],
                "diff_stat": { "files": 0, "insertions": 0, "deletions": 0 },
                "git_status_error": null,
                "git_diff_stat_error": null,
                "worktree_fingerprint": fingerprint,
                "worktree_fingerprint_error": null
            }),
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
        assert_eq!(gates["gates"][0]["freshness_receipt_id"], "receipt_direct");
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

        append_receipt(
            temp.path(),
            json!({
                "id": "receipt_target_tool",
                "session_id": null,
                "plan_id": "plan_1",
                "tool_name": "jig.custom_check",
                "args": {},
                "invoked_make_target": "custom-check",
                "started_at_ms": 100,
                "ended_at_ms": 110,
                "exit_status": 0,
                "stdout_preview": "",
                "stderr_preview": "",
                "changed_paths": [],
                "diff_stat": { "files": 0, "insertions": 0, "deletions": 0 },
                "git_status_error": null,
                "git_diff_stat_error": null,
                "worktree_fingerprint": null,
                "worktree_fingerprint_error": null
            }),
        );
        append_receipt(
            temp.path(),
            json!({
                "id": "receipt_target_batch",
                "session_id": null,
                "plan_id": "plan_1",
                "tool_name": "jig.work_check",
                "args": {
                    "plan_id": "plan_1",
                    "tools": ["jig.custom_check"],
                    "receipt_ids": ["receipt_target_tool"]
                },
                "invoked_make_target": null,
                "started_at_ms": 100,
                "ended_at_ms": 120,
                "exit_status": 0,
                "stdout_preview": "",
                "stderr_preview": "",
                "changed_paths": [],
                "diff_stat": { "files": 0, "insertions": 0, "deletions": 0 },
                "git_status_error": null,
                "git_diff_stat_error": null,
                "worktree_fingerprint": fingerprint,
                "worktree_fingerprint_error": null
            }),
        );
        append_receipt(
            temp.path(),
            json!({
                "id": "receipt_unrelated_batch",
                "session_id": null,
                "plan_id": "plan_1",
                "tool_name": "jig.work_check",
                "args": {
                    "plan_id": "plan_1",
                    "tools": ["jig.custom_check"],
                    "receipt_ids": ["receipt_other_tool"]
                },
                "invoked_make_target": null,
                "started_at_ms": 90,
                "ended_at_ms": 130,
                "exit_status": 0,
                "stdout_preview": "",
                "stderr_preview": "",
                "changed_paths": [],
                "diff_stat": { "files": 0, "insertions": 0, "deletions": 0 },
                "git_status_error": null,
                "git_diff_stat_error": null,
                "worktree_fingerprint": "stale-fingerprint",
                "worktree_fingerprint_error": null
            }),
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
            "receipt_target_batch"
        );
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
}
