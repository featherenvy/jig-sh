use std::path::PathBuf;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::{
    CommandKind, DecisionAddOpts, MigrationAddOpts, PlanAppendOpts, PlanCloseOpts, PlanOpenOpts,
    ReceiptsListOpts, RunTargetOpts, SessionEndOpts, ToolOpts,
};
use crate::context::{ManifestTool, RepoContext};
use crate::state::{
    ReceiptInput, decisions_add, now_ms, plans_append, plans_close, plans_open, receipts_list,
    record_receipt, session_end, session_start,
};

pub(crate) fn dispatch(ctx: &RepoContext, command: CommandKind) -> Result<Value> {
    match command {
        CommandKind::FmtCheck(opts) => {
            execute_make_tool(ctx, "jig.fmt_check", "fmt-check", json!({}), opts.plan_id)
        }
        CommandKind::Clippy(opts) => {
            execute_make_tool(ctx, "jig.clippy", "clippy", json!({}), opts.plan_id)
        }
        CommandKind::Test(opts) => {
            execute_make_tool(ctx, "jig.test", "test", json!({}), opts.plan_id)
        }
        CommandKind::TestLocked(opts) => execute_make_tool(
            ctx,
            "jig.test_locked",
            "test-rust-locked",
            json!({}),
            opts.plan_id,
        ),
        CommandKind::SqlxCheck(opts) => {
            execute_make_tool(ctx, "jig.sqlx_check", "sqlx-check", json!({}), opts.plan_id)
        }
        CommandKind::SchemaCheck(opts) => execute_make_tool(
            ctx,
            "jig.schema_check",
            "schema-check",
            json!({}),
            opts.plan_id,
        ),
        CommandKind::SchemaDump(opts) => execute_make_tool(
            ctx,
            "jig.schema_dump",
            "schema-dump",
            json!({}),
            opts.plan_id,
        ),
        CommandKind::MigrationAdd(opts) => execute_make_tool(
            ctx,
            "jig.migration_add",
            "migration-add",
            json!({ "name": opts.name }),
            opts.tool.plan_id,
        )
        .map(|value| {
            let name = value["args"]["name"].clone();
            json!({
                "ok": true,
                "tool": "jig.migration_add",
                "name": name,
                "result": value["result"],
                "receipt_id": value["receipt_id"],
            })
        }),
        CommandKind::ContractCheck(opts) => execute_make_tool(
            ctx,
            "jig.contract_check",
            "contract-check",
            json!({}),
            opts.plan_id,
        ),
        CommandKind::RunTarget(opts) => execute_make_tool(
            ctx,
            "jig.run_target",
            &opts.name,
            json!({ "name": opts.name }),
            opts.tool.plan_id,
        ),
        CommandKind::SessionStart => session_start(ctx),
        CommandKind::SessionEnd(opts) => session_end(ctx, opts),
        CommandKind::PlansOpen(opts) => plans_open(ctx, opts),
        CommandKind::PlansAppend(opts) => plans_append(ctx, opts),
        CommandKind::PlansClose(opts) => plans_close(ctx, opts),
        CommandKind::ReceiptsList(opts) => receipts_list(ctx, opts),
        CommandKind::DecisionsAdd(opts) => decisions_add(ctx, opts),
        CommandKind::Init(_) | CommandKind::Adopt(_) | CommandKind::Update(_) => unreachable!(),
        CommandKind::Mcp => unreachable!(),
    }
}

pub(crate) fn tool_specs(ctx: &RepoContext) -> &[ManifestTool] {
    ctx.tool_specs()
}

pub(crate) fn call_tool(ctx: &RepoContext, name: &str, args: Value) -> Result<Value> {
    let args_obj = args
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);

    let command = match name {
        "jig.fmt_check" => CommandKind::FmtCheck(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.clippy" => CommandKind::Clippy(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.test" => CommandKind::Test(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.test_locked" => CommandKind::TestLocked(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.sqlx_check" => CommandKind::SqlxCheck(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.schema_check" => CommandKind::SchemaCheck(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.schema_dump" => CommandKind::SchemaDump(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.migration_add" => CommandKind::MigrationAdd(MigrationAddOpts {
            name: required_string_arg(&args_obj, "name")?,
            tool: ToolOpts {
                plan_id: string_arg(&args_obj, "plan_id"),
            },
        }),
        "jig.contract_check" => CommandKind::ContractCheck(ToolOpts {
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        "jig.run_target" => CommandKind::RunTarget(RunTargetOpts {
            name: required_string_arg(&args_obj, "name")?,
            tool: ToolOpts {
                plan_id: string_arg(&args_obj, "plan_id"),
            },
        }),
        "jig.session_start" => CommandKind::SessionStart,
        "jig.session_end" => CommandKind::SessionEnd(SessionEndOpts {
            session_id: string_arg(&args_obj, "session_id"),
            outcome: string_arg(&args_obj, "outcome"),
        }),
        "jig.plans_open" => CommandKind::PlansOpen(PlanOpenOpts {
            title: required_string_arg(&args_obj, "title")?,
            body: string_arg(&args_obj, "body"),
            body_file: string_arg(&args_obj, "body_file").map(PathBuf::from),
        }),
        "jig.plans_append" => CommandKind::PlansAppend(PlanAppendOpts {
            plan_id: required_string_arg(&args_obj, "plan_id")?,
            body: string_arg(&args_obj, "body"),
            body_file: string_arg(&args_obj, "body_file").map(PathBuf::from),
        }),
        "jig.plans_close" => CommandKind::PlansClose(PlanCloseOpts {
            plan_id: required_string_arg(&args_obj, "plan_id")?,
            resolution: string_arg(&args_obj, "resolution"),
        }),
        "jig.receipts_list" => CommandKind::ReceiptsList(ReceiptsListOpts {
            session_id: string_arg(&args_obj, "session_id"),
            plan_id: string_arg(&args_obj, "plan_id"),
            limit: usize_arg(&args_obj, "limit").unwrap_or(20),
        }),
        "jig.decisions_add" => CommandKind::DecisionsAdd(DecisionAddOpts {
            title: required_string_arg(&args_obj, "title")?,
            selected_option: required_string_arg(&args_obj, "selected_option")?,
            rationale: required_string_arg(&args_obj, "rationale")?,
            alternatives: string_list_arg(&args_obj, "alternatives"),
            plan_id: string_arg(&args_obj, "plan_id"),
        }),
        other => bail!("Unsupported tool: {other}"),
    };

    dispatch(ctx, command)
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
        },
    )?;

    if !output.status.success() {
        bail!("{tool_name} failed with status {exit_status}\nstdout:\n{stdout}\nstderr:\n{stderr}");
    }

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

    if target == "migration-add" {
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("migration-add requires a name argument"))?;
        command.arg(format!("NAME={name}"));
    }

    command
        .output()
        .with_context(|| format!("Failed to run make {target}"))
}

fn required_string_arg(map: &serde_json::Map<String, Value>, key: &str) -> Result<String> {
    string_arg(map, key).ok_or_else(|| anyhow!("Missing required argument: {key}"))
}

fn string_arg(map: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn usize_arg(map: &serde_json::Map<String, Value>, key: &str) -> Option<usize> {
    map.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn string_list_arg(map: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}
