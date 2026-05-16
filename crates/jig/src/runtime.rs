use std::borrow::Cow;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::{AgentMapCommand, CheckCommand, CommandKind};
use crate::context::{ManifestTool, RepoContext};
use crate::policy::{
    AgentMapInput, MigrationImmutabilityInput, PolicyCheckCommand, PolicyDirectCommand,
    RustFileLocInput, SqlxTodoInput,
};
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::{self, JsonObject, MemoryTool, args, string_arg, tool};

mod agent;
mod requests;
mod work;

pub(crate) fn dispatch(ctx: &RepoContext, command: CommandKind) -> Result<Value> {
    match command {
        CommandKind::Bootstrap(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::BOOTSTRAP,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CommandKind::Check(command) => dispatch_check(ctx, command),
        CommandKind::SchemaDump(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::SCHEMA_DUMP,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CommandKind::MigrationAdd(opts) => {
            let record_receipt = opts.tool.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::MIGRATION_ADD,
                json!({ args::NAME: opts.name }),
                opts.tool.plan_id,
                record_receipt,
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
            })
        }
        CommandKind::AgentMap(AgentMapCommand::Generate(opts)) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::AgentMapGenerate(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        CommandKind::GenerateSqlxUncheckedQueriesTodo(opts) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::GenerateSqlxUncheckedQueriesTodo(SqlxTodoInput {
                output: opts.output,
            }),
        ),
        CommandKind::Dev(opts) => crate::dev_proxy::commands::dev(ctx, opts),
        CommandKind::RunTarget(opts) => {
            let record_receipt = opts.tool.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::RUN_TARGET,
                json!({ args::NAME: opts.name }),
                opts.tool.plan_id,
                record_receipt,
            )
        }
        CommandKind::Proxy(command) => crate::dev_proxy::commands::proxy(ctx, command),
        CommandKind::Agent(command) => agent::dispatch(ctx, command),
        CommandKind::Work(command) => work::dispatch(ctx, command),
        CommandKind::Init(_) | CommandKind::Adopt(_) | CommandKind::Update(_) => unreachable!(),
        CommandKind::Mcp => unreachable!(),
    }
}

fn dispatch_check(ctx: &RepoContext, command: CheckCommand) -> Result<Value> {
    match command {
        CheckCommand::Fmt(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::FMT_CHECK,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CheckCommand::Clippy(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(ctx, tool::CLIPPY, json!({}), opts.plan_id, record_receipt)
        }
        CheckCommand::Test(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(ctx, tool::TEST, json!({}), opts.plan_id, record_receipt)
        }
        CheckCommand::TestLocked(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::TEST_LOCKED,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CheckCommand::Sqlx(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::SQLX_CHECK,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CheckCommand::Schema(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::SCHEMA_CHECK,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CheckCommand::Contract(opts) => {
            let record_receipt = opts.record_receipt();
            execute_manifest_tool(
                ctx,
                tool::CONTRACT_CHECK,
                json!({}),
                opts.plan_id,
                record_receipt,
            )
        }
        CheckCommand::AgentMap(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::AgentMap(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        CheckCommand::AgentGuides => crate::policy::run_check(ctx, PolicyCheckCommand::AgentGuides),
        CheckCommand::RustFileLoc(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::RustFileLoc(RustFileLocInput {
                staged: opts.staged,
                changed_against: opts.changed_against,
                all: opts.all,
            }),
        ),
        CheckCommand::NoModRs => crate::policy::run_check(ctx, PolicyCheckCommand::NoModRs),
        CheckCommand::MigrationImmutability(opts) => crate::policy::run_check(
            ctx,
            PolicyCheckCommand::MigrationImmutability(MigrationImmutabilityInput {
                changed_against: opts.changed_against,
            }),
        ),
        CheckCommand::SqlxUncheckedNonTest => {
            crate::policy::run_check(ctx, PolicyCheckCommand::SqlxUncheckedNonTest)
        }
    }
}

pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    let args_obj = args.as_object().cloned().unwrap_or_default();

    match ctx.tool_spec(name) {
        Some(tool) if tool_defs::is_execution_tool(tool) => {
            return call_manifest_tool(ctx, tool, &args_obj);
        }
        _ => {}
    }

    // MCP dispatch is intentionally allowlisted here. CLI-only dev/proxy
    // commands can start processes, install services, or mutate trust stores
    // and must not become agent-callable by adding names to tool_defs.
    match MemoryTool::from_name(name) {
        Some(MemoryTool::AgentDoctor) => agent::doctor(ctx),
        Some(MemoryTool::Goal) => work::goal_from_args(ctx, args),
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

fn call_manifest_tool(
    ctx: &RepoContext,
    tool: &ManifestTool,
    args_obj: &JsonObject,
) -> Result<Value> {
    let plan_id = string_arg(args_obj, args::PLAN_ID);
    let args = tool_defs::execution_tool_args(tool, args_obj)?;

    // MCP execution tools are evidence-producing by design; the CLI-only
    // --no-receipt escape hatch is intentionally not part of the tool schema.
    execute_manifest_tool(ctx, &tool.name, args, plan_id, true)
}

fn execute_manifest_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
) -> Result<Value> {
    execute_manifest_tool_with_options(ctx, tool_name, args, plan_id, record_receipt, true)
}

fn execute_manifest_tool_without_worktree_fingerprint(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
) -> Result<Value> {
    execute_manifest_tool_with_options(ctx, tool_name, args, plan_id, true, false)
}

fn execute_manifest_tool_with_options(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let tool = ctx
        .tool_spec(tool_name)
        .ok_or_else(|| anyhow!("Tool is not declared in .agent/jig-contract.json: {tool_name}"))?;
    if tool_defs::is_make_tool(tool) {
        let target = match tool.target.as_deref() {
            Some(target) => Cow::Borrowed(target),
            None => args
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{tool_name} requires a name argument"))?
                .to_string()
                .into(),
        };

        return execute_make_tool(
            ctx,
            &tool.name,
            target.as_ref(),
            args,
            plan_id,
            record_receipt,
            collect_worktree_fingerprint,
        );
    }

    if tool_defs::is_native_tool(tool) {
        return execute_native_tool(
            ctx,
            &tool.name,
            args,
            plan_id,
            record_receipt,
            collect_worktree_fingerprint,
        );
    }

    if tool_defs::is_command_tool(tool) {
        let command_key = tool
            .command
            .as_deref()
            .ok_or_else(|| anyhow!("Command-backed tool is missing command: {tool_name}"))?;
        let command = ctx.command_for_key(command_key)?;
        return execute_command_tool(
            ctx,
            &tool.name,
            command_key,
            command,
            args,
            plan_id,
            record_receipt,
            collect_worktree_fingerprint,
        );
    }

    bail!("Unsupported tool kind '{}' for {tool_name}", tool.kind)
}

fn execute_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let started = now_ms();
    let output = match tool_name {
        tool::CONTRACT_CHECK => crate::policy::contract_check(ctx),
        tool::MIGRATION_ADD => {
            let name = args
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name)
        }
        tool::SCHEMA_CHECK => crate::policy::schema_check(ctx),
        _ => bail!("Unsupported native tool: {tool_name}"),
    }?;
    let ended = now_ms();

    let receipt_result = maybe_record_receipt(
        ctx,
        record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_make_target: None,
            invoked_command_key: None,
            plan_id,
            started_at_ms: started,
            ended_at_ms: ended,
            exit_status: output.exit_status,
            stdout: &output.stdout,
            stderr: &output.stderr,
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint,
            worktree_fingerprint_override: None,
        },
    );

    let receipt_id = receipt_id_or_preserve_tool_error(
        (output.exit_status != 0).then(|| {
            format!(
                "{tool_name} failed with status {}\nstdout:\n{}\nstderr:\n{}",
                output.exit_status, output.stdout, output.stderr
            )
        }),
        receipt_result,
    )?;

    Ok(json!({
        "ok": true,
        "tool": tool_name,
        "args": args,
        "result": {
            "exit_status": output.exit_status,
            "stdout": output.stdout,
            "stderr": output.stderr,
        },
        "receipt_id": receipt_id,
    }))
}

fn receipt_id_or_preserve_tool_error(
    tool_failure: Option<String>,
    receipt_result: Result<Option<String>>,
) -> Result<Option<String>> {
    if let Some(tool_failure) = tool_failure {
        match receipt_result {
            Ok(_) => bail!("{tool_failure}"),
            Err(receipt_error) => {
                bail!("{tool_failure}\nreceipt recording also failed:\n{receipt_error:#}")
            }
        }
    } else {
        receipt_result
    }
}

fn execute_make_tool(
    ctx: &RepoContext,
    tool_name: &str,
    target: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let started = now_ms();
    let output = run_make(ctx, target, &args)?;
    let ended = now_ms();
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let receipt_result = maybe_record_receipt(
        ctx,
        record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_make_target: Some(target.to_string()),
            invoked_command_key: None,
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
    );

    let receipt_id = receipt_id_or_preserve_tool_error(
        (!output.status.success()).then(|| {
            format!(
                "{tool_name} failed with status {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        }),
        receipt_result,
    )?;

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

fn execute_command_tool(
    ctx: &RepoContext,
    tool_name: &str,
    command_key: &str,
    command_text: &str,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let started = now_ms();
    let output = run_configured_command(ctx, tool_name, command_text, &args)?;
    let ended = now_ms();
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let receipt_result = maybe_record_receipt(
        ctx,
        record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
            invoked_make_target: None,
            invoked_command_key: Some(command_key.to_string()),
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
    );

    let receipt_id = receipt_id_or_preserve_tool_error(
        (!output.status.success()).then(|| {
            format!(
                "{tool_name} failed with status {exit_status}\ncommand key: {command_key}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        }),
        receipt_result,
    )?;

    Ok(json!({
        "ok": true,
        "tool": tool_name,
        "command_key": command_key,
        "args": args,
        "result": {
            "exit_status": exit_status,
            "stdout": stdout,
            "stderr": stderr,
        },
        "receipt_id": receipt_id,
    }))
}

fn maybe_record_receipt(
    ctx: &RepoContext,
    should_record_receipt: bool,
    input: ReceiptInput<'_>,
) -> Result<Option<String>> {
    if should_record_receipt {
        record_receipt(ctx, input).map(Some)
    } else {
        Ok(None)
    }
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

fn run_configured_command(
    ctx: &RepoContext,
    tool_name: &str,
    command_text: &str,
    args: &Value,
) -> Result<Output> {
    let mut command = Command::new("bash");
    command.current_dir(ctx.root()).arg("-c").arg(command_text);

    if tool_name == tool::MIGRATION_ADD {
        let name = args
            .get(args::NAME)
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
        command.env("NAME", name);
    }

    command
        .output()
        .with_context(|| format!("Failed to run configured command for {tool_name}"))
}

#[cfg(test)]
mod tests;
