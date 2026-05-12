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
        Some(MemoryTool::Start) => work::start_from_args(ctx, args),
        Some(MemoryTool::Append) => work::append_from_args(ctx, args),
        Some(MemoryTool::Check) => work::check_from_args(ctx, args),
        Some(MemoryTool::Gates) => work::gates_from_args(ctx, args),
        Some(MemoryTool::Decide) => work::decide_from_args(ctx, args),
        Some(MemoryTool::Receipts) => work::receipts_from_args(ctx, args),
        Some(MemoryTool::Status) => crate::state::state_summary(ctx),
        Some(MemoryTool::Finish) => work::finish_from_args(ctx, args),
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
mod tests;
