use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use jig_contract::{ManifestTool, NativeToolKind};
use serde_json::{Value, json};

use crate::command::{AgentMapCommand, CheckCommand, RuntimeCommand, ToolRequest};
use crate::context::RepoContext;
use crate::policy::{
    AgentMapInput, MigrationImmutabilityInput, NativeToolOutput, PolicyCheckCommand,
    PolicyDirectCommand, RustFileLocInput, SqlxTodoInput,
};
use crate::state::{ReceiptInput, now_ms, record_receipt};
use crate::tool_defs::{self, JsonObject, MemoryTool, args, kind, string_arg, tool};

mod agent;
mod vault;
mod work;

pub(crate) fn dispatch(ctx: &RepoContext, command: RuntimeCommand) -> Result<Value> {
    match command {
        RuntimeCommand::Bootstrap(opts) => {
            execute_manifest_tool_request(ctx, tool::BOOTSTRAP, json!({}), opts)
        }
        RuntimeCommand::Check(command) => dispatch_check(ctx, command),
        RuntimeCommand::SchemaDump(opts) => {
            execute_manifest_tool_request(ctx, tool::SCHEMA_DUMP, json!({}), opts)
        }
        RuntimeCommand::MigrationAdd(opts) => {
            let (plan_id, record_receipt) = opts.tool.into_parts();
            execute_manifest_tool(
                ctx,
                tool::MIGRATION_ADD,
                json!({ args::NAME: opts.name }),
                plan_id,
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
        RuntimeCommand::AgentMap(AgentMapCommand::Generate(opts)) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::AgentMapGenerate(AgentMapInput {
                map_path: opts.map_path,
            }),
        ),
        RuntimeCommand::GenerateSqlxUncheckedQueriesTodo(opts) => crate::policy::run_direct(
            ctx,
            PolicyDirectCommand::GenerateSqlxUncheckedQueriesTodo(SqlxTodoInput {
                output: opts.output,
            }),
        ),
        RuntimeCommand::Dev(opts) => crate::dev_proxy::commands::dev(ctx, opts),
        RuntimeCommand::Proxy(command) => crate::dev_proxy::commands::proxy(ctx, command),
        RuntimeCommand::Agent(command) => agent::dispatch(ctx, command),
        RuntimeCommand::Work(command) => work::dispatch(ctx, command),
    }
}

pub(crate) fn dispatch_vault(command: crate::command::VaultCommand) -> Result<Value> {
    vault::dispatch(command)
}

pub(crate) fn capture_vault_passphrase() -> Result<()> {
    // SAFETY: Callers must invoke this before starting background threads in the
    // process; `runtime::vault` clears the captured environment variable.
    vault::capture_passphrase()
}

pub(crate) fn capture_new_vault_passphrase() -> Result<()> {
    // SAFETY: Callers must invoke this before starting background threads in the
    // process; `runtime::vault` clears the captured environment variable.
    vault::capture_new_passphrase()
}

fn dispatch_check(ctx: &RepoContext, command: CheckCommand) -> Result<Value> {
    match command {
        CheckCommand::Fmt(opts) => {
            execute_manifest_tool_request(ctx, tool::FMT_CHECK, json!({}), opts)
        }
        CheckCommand::Clippy(opts) => {
            execute_manifest_tool_request(ctx, tool::CLIPPY, json!({}), opts)
        }
        CheckCommand::Test(opts) => execute_manifest_tool_request(ctx, tool::TEST, json!({}), opts),
        CheckCommand::TestLocked(opts) => {
            execute_manifest_tool_request(ctx, tool::TEST_LOCKED, json!({}), opts)
        }
        CheckCommand::TypeScriptLint(opts) => {
            execute_manifest_tool_request(ctx, tool::TYPESCRIPT_LINT, json!({}), opts)
        }
        CheckCommand::TypeScriptTypecheck(opts) => {
            execute_manifest_tool_request(ctx, tool::TYPESCRIPT_TYPECHECK, json!({}), opts)
        }
        CheckCommand::TypeScriptBuild(opts) => {
            execute_manifest_tool_request(ctx, tool::TYPESCRIPT_BUILD, json!({}), opts)
        }
        CheckCommand::TypeScriptCoverage(opts) => {
            execute_manifest_tool_request(ctx, tool::TYPESCRIPT_COVERAGE, json!({}), opts)
        }
        CheckCommand::Sqlx(opts) => {
            execute_manifest_tool_request(ctx, tool::SQLX_CHECK, json!({}), opts)
        }
        CheckCommand::Schema(opts) => {
            execute_manifest_tool_request(ctx, tool::SCHEMA_CHECK, json!({}), opts)
        }
        CheckCommand::Contract(opts) => {
            execute_manifest_tool_request(ctx, tool::CONTRACT_CHECK, json!({}), opts)
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

fn execute_manifest_tool_request(
    ctx: &RepoContext,
    tool_name: &str,
    args: Value,
    request: ToolRequest,
) -> Result<Value> {
    let (plan_id, record_receipt) = request.into_parts();
    execute_manifest_tool(ctx, tool_name, args, plan_id, record_receipt)
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
        Some(MemoryTool::Evidence) => work::evidence_from_args(ctx, args),
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
        .ok_or_else(|| anyhow!("{}", undeclared_tool_message(ctx, tool_name)))?;
    match tool.kind.as_str() {
        kind::NATIVE => execute_native_tool(
            ctx,
            &tool.name,
            args,
            plan_id,
            record_receipt,
            collect_worktree_fingerprint,
        ),
        kind::COMMAND => {
            let command_key = tool
                .command
                .as_deref()
                .ok_or_else(|| anyhow!("Command-backed tool is missing command: {tool_name}"))?;
            let command = ctx.command_for_key(command_key)?;
            execute_command_tool(
                ctx,
                CommandToolInvocation {
                    tool_name: &tool.name,
                    command_key,
                    command_text: command,
                },
                args,
                plan_id,
                record_receipt,
                collect_worktree_fingerprint,
            )
        }
        _ => bail!("Unsupported tool kind '{}' for {tool_name}", tool.kind),
    }
}

fn undeclared_tool_message(ctx: &RepoContext, tool_name: &str) -> String {
    if let Some(message) = jig_features::unavailable_tool_message(ctx, tool_name) {
        message
    } else {
        format!("Tool is not declared in .agent/jig-contract.json: {tool_name}")
    }
}

fn run_native_tool(
    ctx: &RepoContext,
    tool_name: &str,
    args_value: &Value,
) -> Result<NativeToolOutput> {
    match jig_features::native_tool_kind(tool_name)
        .ok_or_else(|| anyhow!("Unsupported native tool: {tool_name}"))?
    {
        NativeToolKind::ContractCheck => crate::policy::contract_check(ctx),
        NativeToolKind::MigrationAdd => {
            let name = args_value
                .get(args::NAME)
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{} requires a name argument", tool::MIGRATION_ADD))?;
            crate::policy::migration_add(ctx, name)
        }
        NativeToolKind::SchemaCheck => crate::policy::schema_check(ctx),
        _ => bail!("Unsupported native tool kind for {tool_name}"),
    }
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
    let output = run_native_tool(ctx, tool_name, &args)?;
    let ended = now_ms();

    let receipt_result = maybe_record_receipt(
        ctx,
        record_receipt,
        ReceiptInput {
            tool_name,
            args: args.clone(),
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

struct CommandToolInvocation<'a> {
    tool_name: &'a str,
    command_key: &'a str,
    command_text: &'a str,
}

fn execute_command_tool(
    ctx: &RepoContext,
    invocation: CommandToolInvocation<'_>,
    args: Value,
    plan_id: Option<String>,
    record_receipt: bool,
    collect_worktree_fingerprint: bool,
) -> Result<Value> {
    let started = now_ms();
    let output = run_configured_command(ctx, invocation.tool_name, invocation.command_text, &args)?;
    let ended = now_ms();
    let exit_status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    let receipt_result = maybe_record_receipt(
        ctx,
        record_receipt,
        ReceiptInput {
            tool_name: invocation.tool_name,
            args: args.clone(),
            invoked_command_key: Some(invocation.command_key.to_string()),
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
            let tool_name = invocation.tool_name;
            let command_key = invocation.command_key;
            format!(
                "{tool_name} failed with status {exit_status}\ncommand key: {command_key}\nstdout:\n{stdout}\nstderr:\n{stderr}"
            )
        }),
        receipt_result,
    )?;

    Ok(json!({
        "ok": true,
        "tool": invocation.tool_name,
        "command_key": invocation.command_key,
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
