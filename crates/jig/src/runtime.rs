use std::borrow::Cow;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::{CommandKind, DEFAULT_RECEIPTS_LIMIT};
use crate::context::{ManifestTool, RepoContext};
use crate::process::require_success;
use crate::state::{
    ReceiptInput, decisions_add, now_ms, plans_append, plans_close, plans_open, receipts_list,
    record_receipt, session_end, session_start, state_summary,
};
use crate::tool_defs::{self, JsonObject, MemoryTool, args, string_arg, tool};

mod requests;

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
        CommandKind::SessionStart => session_start(ctx),
        CommandKind::SessionEnd(opts) => session_end(ctx, opts.into()),
        CommandKind::PlansOpen(opts) => plans_open(ctx, opts.into()),
        CommandKind::PlansAppend(opts) => plans_append(ctx, opts.into()),
        CommandKind::PlansClose(opts) => plans_close(ctx, opts.into()),
        CommandKind::ReceiptsList(opts) => receipts_list(ctx, opts.into()),
        CommandKind::StateSummary => state_summary(ctx),
        CommandKind::DecisionsAdd(opts) => decisions_add(ctx, opts.into()),
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
        Some(MemoryTool::SessionStart) => session_start(ctx),
        Some(MemoryTool::SessionEnd) => {
            session_end(ctx, requests::session_end_request_from_args(&args_obj))
        }
        Some(MemoryTool::PlansOpen) => {
            plans_open(ctx, requests::plan_open_request_from_args(&args_obj)?)
        }
        Some(MemoryTool::PlansAppend) => {
            plans_append(ctx, requests::plan_append_request_from_args(&args_obj)?)
        }
        Some(MemoryTool::PlansClose) => {
            plans_close(ctx, requests::plan_close_request_from_args(&args_obj)?)
        }
        Some(MemoryTool::ReceiptsList) => receipts_list(
            ctx,
            requests::receipt_list_filter_from_args(&args_obj, DEFAULT_RECEIPTS_LIMIT),
        ),
        Some(MemoryTool::StateSummary) => state_summary(ctx),
        Some(MemoryTool::DecisionsAdd) => {
            decisions_add(ctx, requests::decision_add_request_from_args(&args_obj)?)
        }
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

    execute_make_tool(ctx, &tool.name, target.as_ref(), args, plan_id)
}

fn execute_make_tool(
    ctx: &RepoContext,
    tool_name: &str,
    target: &str,
    args: Value,
    plan_id: Option<String>,
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
    use std::path::Path;

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
}
