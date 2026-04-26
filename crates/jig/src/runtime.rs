use std::borrow::Cow;
use std::path::PathBuf;
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};

use crate::cli::{
    CommandKind, DEFAULT_RECEIPTS_LIMIT, DecisionAddOpts, PlanAppendOpts, PlanCloseOpts,
    PlanOpenOpts, ReceiptsListOpts, SessionEndOpts,
};
use crate::context::{ManifestTool, RepoContext};
use crate::process::require_success;
use crate::state::{
    ReceiptInput, decisions_add, now_ms, plans_append, plans_close, plans_open, receipts_list,
    record_receipt, session_end, session_start, state_summary,
};

pub(crate) fn dispatch(ctx: &RepoContext, command: CommandKind) -> Result<Value> {
    match command {
        CommandKind::FmtCheck(opts) => {
            execute_manifest_make_tool(ctx, "jig.fmt_check", json!({}), opts.plan_id)
        }
        CommandKind::Clippy(opts) => {
            execute_manifest_make_tool(ctx, "jig.clippy", json!({}), opts.plan_id)
        }
        CommandKind::Test(opts) => {
            execute_manifest_make_tool(ctx, "jig.test", json!({}), opts.plan_id)
        }
        CommandKind::TestLocked(opts) => {
            execute_manifest_make_tool(ctx, "jig.test_locked", json!({}), opts.plan_id)
        }
        CommandKind::SqlxCheck(opts) => {
            execute_manifest_make_tool(ctx, "jig.sqlx_check", json!({}), opts.plan_id)
        }
        CommandKind::SchemaCheck(opts) => {
            execute_manifest_make_tool(ctx, "jig.schema_check", json!({}), opts.plan_id)
        }
        CommandKind::SchemaDump(opts) => {
            execute_manifest_make_tool(ctx, "jig.schema_dump", json!({}), opts.plan_id)
        }
        CommandKind::MigrationAdd(opts) => execute_manifest_make_tool(
            ctx,
            "jig.migration_add",
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
        CommandKind::ContractCheck(opts) => {
            execute_manifest_make_tool(ctx, "jig.contract_check", json!({}), opts.plan_id)
        }
        CommandKind::RunTarget(opts) => execute_manifest_make_tool(
            ctx,
            "jig.run_target",
            json!({ "name": opts.name }),
            opts.tool.plan_id,
        ),
        CommandKind::SessionStart => session_start(ctx),
        CommandKind::SessionEnd(opts) => session_end(ctx, opts),
        CommandKind::PlansOpen(opts) => plans_open(ctx, opts),
        CommandKind::PlansAppend(opts) => plans_append(ctx, opts),
        CommandKind::PlansClose(opts) => plans_close(ctx, opts),
        CommandKind::ReceiptsList(opts) => receipts_list(ctx, opts),
        CommandKind::StateSummary => state_summary(ctx),
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

    if let Some(tool) = ctx.tool_spec(name)
        && tool.kind == "make"
    {
        return call_manifest_make_tool(ctx, tool, &args_obj);
    }

    let command = match name {
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
        "jig.receipts_list" => CommandKind::ReceiptsList(receipts_list_opts(&args_obj)),
        "jig.state_summary" => CommandKind::StateSummary,
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

fn call_manifest_make_tool(
    ctx: &RepoContext,
    tool: &ManifestTool,
    args_obj: &serde_json::Map<String, Value>,
) -> Result<Value> {
    let plan_id = string_arg(args_obj, "plan_id");
    let args = make_tool_args(tool, args_obj)?;

    execute_manifest_make_tool(ctx, &tool.name, args, plan_id)
}

fn make_tool_args(tool: &ManifestTool, args_obj: &serde_json::Map<String, Value>) -> Result<Value> {
    if tool.name == "jig.migration_add" || tool.target.is_none() {
        return Ok(json!({ "name": required_string_arg(args_obj, "name")? }));
    }

    Ok(json!({}))
}

fn receipts_list_opts(args_obj: &serde_json::Map<String, Value>) -> ReceiptsListOpts {
    ReceiptsListOpts {
        session_id: string_arg(args_obj, "session_id"),
        plan_id: string_arg(args_obj, "plan_id"),
        tool_name: string_arg(args_obj, "tool_name"),
        failed_only: bool_arg(args_obj, "failed_only").unwrap_or_default(),
        limit: usize_arg(args_obj, "limit").unwrap_or(DEFAULT_RECEIPTS_LIMIT),
    }
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
    if tool.kind != "make" {
        bail!("Tool is not a make-backed tool: {tool_name}");
    }

    let target = match tool.target.as_deref() {
        Some(target) => Cow::Borrowed(target),
        None => args
            .get("name")
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

fn bool_arg(map: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use tempfile::tempdir;

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
                "memory_schema_version": 1,
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
