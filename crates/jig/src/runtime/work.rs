use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::command::{
    WorkAppendRequest, WorkCheckRequest, WorkCommand, WorkDecisionRequest, WorkFinishRequest,
    WorkGatesRequest, WorkGoalRequest, WorkReceiptsRequest, WorkStartRequest,
};
use crate::context::{RepoContext, WorkGateConfig};
use crate::state::{
    DecisionAddRequest, PlanAppendRequest, PlanCloseRequest, PlanOpenRequest, ReceiptInput,
    ReceiptListFilter, SessionEndRequest, current_session, current_worktree_fingerprint,
    decisions_add, ensure_plan_exists, ensure_plan_is_open, latest_plan_tool_receipt,
    latest_plan_work_check_receipt_for_tool, now_ms, plans_append, plans_close, plans_open,
    receipts_list, record_receipt, session_end, session_start, state_summary,
};
use crate::tool_defs::{self, tool};

use super::execute_manifest_tool_without_worktree_fingerprint;

struct GoalHarness {
    objective: String,
    success: String,
    validations: Vec<String>,
    constraints: Vec<String>,
    checkpoints: Vec<String>,
    title: String,
    notes: Option<String>,
}

impl GoalHarness {
    fn from_request(request: WorkGoalRequest) -> Result<Self> {
        let objective = trimmed_required_text("--objective", &request.objective)?;
        let success = trimmed_required_text("--success", &request.success)?;
        let validations = clean_provided_items("--validation", &request.validations)?;
        if validations.is_empty() {
            bail!("At least one non-empty --validation is required for a goal harness.");
        }
        let constraints = clean_provided_items("--constraint", &request.constraints)?;

        let checkpoints = if request.checkpoints.is_empty() {
            default_checkpoints()
        } else {
            clean_provided_items("--checkpoint", &request.checkpoints)?
        };
        let title = request
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| goal_title(&single_line_text(&objective)));
        let notes = request
            .notes
            .as_deref()
            .map(str::trim)
            .filter(|notes| !notes.is_empty())
            .map(str::to_string);

        Ok(Self {
            objective,
            success,
            validations,
            constraints,
            checkpoints,
            title,
            notes,
        })
    }
}

impl From<WorkStartRequest> for PlanOpenRequest {
    fn from(request: WorkStartRequest) -> Self {
        Self {
            title: request.title,
            body: request.body,
            body_file: request.body_file,
        }
    }
}

impl From<WorkAppendRequest> for PlanAppendRequest {
    fn from(request: WorkAppendRequest) -> Self {
        Self {
            plan_id: request.plan_id,
            body: request.body,
            body_file: request.body_file,
        }
    }
}

impl From<WorkDecisionRequest> for DecisionAddRequest {
    fn from(request: WorkDecisionRequest) -> Self {
        Self {
            title: request.title,
            selected_option: request.selected_option,
            rationale: request.rationale,
            alternatives: request.alternatives,
            plan_id: request.plan_id,
        }
    }
}

impl From<WorkReceiptsRequest> for ReceiptListFilter {
    fn from(request: WorkReceiptsRequest) -> Self {
        Self {
            session_id: request.session_id,
            plan_id: request.plan_id,
            tool_name: request.tool_name,
            failed_only: request.failed_only,
            limit: request.limit,
        }
    }
}

impl From<&WorkFinishRequest> for PlanCloseRequest {
    fn from(request: &WorkFinishRequest) -> Self {
        Self {
            plan_id: request.plan_id.clone(),
            resolution: request.resolution.clone(),
        }
    }
}

pub(super) fn dispatch(ctx: &RepoContext, command: WorkCommand) -> Result<Value> {
    match command {
        WorkCommand::Goal(opts) => goal(ctx, opts),
        WorkCommand::Start(opts) => start(ctx, opts.into()),
        WorkCommand::Append(opts) => plans_append(ctx, opts.into()),
        WorkCommand::Check(opts) => check(ctx, opts),
        WorkCommand::Gates(opts) => gates(ctx, opts),
        WorkCommand::Decide(opts) => decisions_add(ctx, opts.into()),
        WorkCommand::Receipts(opts) => receipts_list(ctx, opts.into()),
        WorkCommand::Status => state_summary(ctx),
        WorkCommand::Finish(opts) => finish(ctx, opts),
    }
}

pub(super) fn goal(ctx: &RepoContext, request: WorkGoalRequest) -> Result<Value> {
    let goal = GoalHarness::from_request(request)?;

    let body = goal_body(ctx, &goal);
    let output = start(
        ctx,
        PlanOpenRequest {
            title: goal.title.clone(),
            body: Some(body),
            body_file: None,
        },
    )?;

    let plan_id = output["plan"]["plan_id"]
        .as_str()
        .ok_or_else(|| anyhow!("Goal harness failed to create a plan id"))?;
    let body_path = output["plan"]["body_path"]
        .as_str()
        .ok_or_else(|| anyhow!("Goal harness failed to create a plan body path"))?;
    let goal_prompt = goal_prompt(plan_id, body_path, &goal);

    Ok(json!({
        "ok": true,
        "session": output["session"],
        "plan": output["plan"],
        "goal_prompt": goal_prompt,
        "commands": {
            "status": "scripts/jig work status",
            "check": format!("scripts/jig work check --plan-id {plan_id}"),
            "gates": format!("scripts/jig work gates --plan-id {plan_id}"),
            "finish": format!("scripts/jig work finish --plan-id {plan_id}")
        }
    }))
}

pub(super) fn start(ctx: &RepoContext, plan: PlanOpenRequest) -> Result<Value> {
    let session = session_start(ctx)?;
    let plan = plans_open(ctx, plan)?;

    Ok(json!({
        "ok": true,
        "session": session,
        "plan": plan,
    }))
}

pub(super) fn check(ctx: &RepoContext, opts: WorkCheckRequest) -> Result<Value> {
    ensure_plan_is_open(ctx, &opts.plan_id)?;
    check_tools(ctx, &opts.plan_id, selected_tools(ctx, &opts.tools)?)
}

pub(super) fn gates(ctx: &RepoContext, opts: WorkGatesRequest) -> Result<Value> {
    ensure_plan_exists(ctx, &opts.plan_id)?;
    gate_status(ctx, &opts.plan_id)
}

pub(super) fn finish(ctx: &RepoContext, opts: WorkFinishRequest) -> Result<Value> {
    // Check before gate evaluation so unknown or already-closed plans report
    // plan-state errors instead of misleading gate failures. plans_close
    // rechecks after gates to preserve the state-layer invariant.
    ensure_plan_is_open(ctx, &opts.plan_id)?;
    ensure_required_gates_passed(ctx, &opts.plan_id)?;

    let plan = plans_close(ctx, (&opts).into())?;
    let session = match current_session(ctx)? {
        Some(_) => Some(session_end(
            ctx,
            session_end_request_for_finish(opts.outcome.or(opts.resolution)),
        )?),
        None => None,
    };

    Ok(json!({
        "ok": true,
        "plan": plan,
        "session": session,
    }))
}

pub(super) fn start_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkStartRequest = request_from_args(args)?;
    start(ctx, request.into())
}

pub(super) fn goal_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    goal(ctx, request_from_args(args)?)
}

pub(super) fn append_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkAppendRequest = request_from_args(args)?;
    plans_append(ctx, request.into())
}

pub(super) fn check_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkCheckRequest = request_from_args(args)?;
    check(ctx, request)
}

pub(super) fn gates_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkGatesRequest = request_from_args(args)?;
    gates(ctx, request)
}

pub(super) fn decide_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkDecisionRequest = request_from_args(args)?;
    decisions_add(ctx, request.into())
}

pub(super) fn receipts_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkReceiptsRequest = request_from_args(args)?;
    receipts_list(ctx, request.into())
}

pub(super) fn finish_from_args(ctx: &RepoContext, args: Value) -> Result<Value> {
    let request: WorkFinishRequest = request_from_args(args)?;
    finish(ctx, request)
}

fn request_from_args<T>(args: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(args).context("Invalid work tool arguments")
}

fn session_end_request_for_finish(outcome: Option<String>) -> SessionEndRequest {
    SessionEndRequest {
        session_id: None,
        outcome,
    }
}

fn selected_tools(ctx: &RepoContext, explicit_tools: &[String]) -> Result<Vec<String>> {
    let tools = if explicit_tools.is_empty() {
        ctx.work_check_tools()
    } else {
        explicit_tools.to_vec()
    };

    if tools.is_empty() {
        bail!("No work check gates configured. Add work.gates to .jig.toml or pass --tool.");
    }

    Ok(tools)
}

fn goal_title(objective: &str) -> String {
    const MAX_TITLE_CHARS: usize = 80;
    const ELLIPSIS: &str = "...";
    let objective = objective.trim();
    if objective.chars().count() <= MAX_TITLE_CHARS {
        return objective.to_string();
    }

    let ellipsis_chars = ELLIPSIS.chars().count();
    let mut title = objective
        .chars()
        .take(MAX_TITLE_CHARS - ellipsis_chars)
        .collect::<String>();
    title.push_str(ELLIPSIS);
    title
}

fn goal_body(ctx: &RepoContext, goal: &GoalHarness) -> String {
    let configured_gates = ctx
        .work_gates()
        .into_iter()
        .map(|gate| match gate.tool {
            Some(tool) => format!("{}: {} ({})", gate.id, gate.kind, tool),
            None => format!("{}: {}", gate.id, gate.kind),
        })
        .collect::<Vec<_>>();

    format!(
        r#"# Goal Harness

## Objective

{objective}

## Verifiable Stopping Condition

{success}

## Validation Loop

{validations}

## Constraints

{constraints}

## Checkpoints

{checkpoints}

## Configured Jig Gates

{configured_gates}

## Progress Log

- Goal harness created. Keep this section short and append dated checkpoints, failed attempts, and validation evidence.

## Notes

{notes}
"#,
        objective = goal.objective.as_str(),
        success = goal.success.as_str(),
        validations = markdown_bullets(&goal.validations, "No validation command specified."),
        constraints = markdown_bullets(&goal.constraints, "No additional constraints specified."),
        checkpoints = markdown_checkboxes(&goal.checkpoints),
        configured_gates = markdown_bullets(&configured_gates, "No work gates configured."),
        notes = goal.notes.as_deref().unwrap_or("No extra notes.")
    )
}

fn goal_prompt(plan_id: &str, body_path: &str, goal: &GoalHarness) -> String {
    format!(
        "/goal Complete the objective in {body_path} without stopping until this verifiable stopping condition is met: {success}. Use {body_path} as the durable progress log, keep changes scoped to the stated constraints, run the validation loop recorded there, inspect gates with `scripts/jig work gates --plan-id {plan_id}`, and stop if blocked by missing product guidance, unsafe permissions, or a validation result that cannot be improved without changing the goal.",
        success = single_line_text(&goal.success),
    )
}

fn trimmed_required_text(flag: &str, value: &str) -> Result<String> {
    let text = value.trim();
    if text.is_empty() {
        bail!("{flag} cannot be empty.");
    }
    Ok(text.to_string())
}

fn single_line_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_provided_items(flag: &str, items: &[String]) -> Result<Vec<String>> {
    let cleaned = clean_items(items);
    if cleaned.len() != items.len() {
        bail!("{flag} values cannot be empty.");
    }
    Ok(cleaned)
}

fn default_checkpoints() -> Vec<String> {
    [
        "Read the relevant AGENTS.md files and repo guidance.",
        "Establish the baseline validation result before risky edits.",
        "Make scoped changes and record each meaningful attempt.",
        "Run the validation loop and inspect gate status.",
        "Finish only after the stopping condition is met.",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn markdown_bullets(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return format!("- {empty}");
    }

    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn markdown_checkboxes(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- [ ] {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn clean_items(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn check_tools(ctx: &RepoContext, plan_id: &str, tools: Vec<String>) -> Result<Value> {
    let started = now_ms();
    let before_fingerprint = current_worktree_fingerprint(ctx);
    let mut results = Vec::with_capacity(tools.len());
    for name in &tools {
        validate_check_tool(ctx, name, "Work check")?;

        results.push(execute_manifest_tool_without_worktree_fingerprint(
            ctx,
            name,
            json!({}),
            Some(plan_id.to_string()),
        )?);
    }
    let receipt_ids = results
        .iter()
        .filter_map(|result| result["receipt_id"].as_str())
        .collect::<Vec<_>>();
    let after_fingerprint = current_worktree_fingerprint(ctx);
    let worktree_fingerprint_override =
        work_check_fingerprint_evidence(&before_fingerprint, &after_fingerprint);
    let receipt_id = record_receipt(
        ctx,
        ReceiptInput {
            tool_name: tool::WORK_CHECK,
            args: json!({
                "plan_id": plan_id,
                "tools": tools,
                "receipt_ids": receipt_ids,
            }),
            invoked_command_key: None,
            plan_id: Some(plan_id.to_string()),
            started_at_ms: started,
            ended_at_ms: now_ms(),
            exit_status: 0,
            stdout: "",
            stderr: "",
            session_override: None,
            collect_git_metadata: true,
            collect_worktree_fingerprint: false,
            worktree_fingerprint_override: Some(worktree_fingerprint_override),
        },
    )?;

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "checks": results,
        "receipt_id": receipt_id,
    }))
}

fn work_check_fingerprint_evidence(
    before: &crate::state::CurrentWorktreeFingerprint,
    after: &crate::state::CurrentWorktreeFingerprint,
) -> std::result::Result<String, String> {
    let before = before
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("before work check", before.error.as_deref()))?;
    let after = after
        .fingerprint
        .as_deref()
        .ok_or_else(|| fingerprint_error("after work check", after.error.as_deref()))?;

    if before == after {
        Ok(after.to_string())
    } else {
        Err(
            "worktree changed during work check; rerun work check after generated changes settle"
                .to_string(),
        )
    }
}

fn fingerprint_error(stage: &str, error: Option<&str>) -> String {
    match error {
        Some(error) => format!("Failed to collect worktree fingerprint {stage}: {error}"),
        None => format!("Failed to collect worktree fingerprint {stage}"),
    }
}

fn gate_status(ctx: &RepoContext, plan_id: &str) -> Result<Value> {
    let mut gates = Vec::new();
    let mut missing_required = Vec::new();
    let mut failed_required = Vec::new();
    let mut stale_required = Vec::new();
    let mut unknown_required = Vec::new();
    let mut unsupported_required = Vec::new();
    let current_fingerprint = current_worktree_fingerprint(ctx);

    for gate in ctx.work_gates() {
        let status = gate_status_value(ctx, plan_id, &gate, &current_fingerprint)?;
        collect_required_gate_failure(
            &gate,
            &status,
            &mut missing_required,
            &mut failed_required,
            &mut stale_required,
            &mut unknown_required,
            &mut unsupported_required,
        );
        gates.push(status);
    }

    let ok = missing_required.is_empty()
        && failed_required.is_empty()
        && stale_required.is_empty()
        && unknown_required.is_empty()
        && unsupported_required.is_empty();

    Ok(json!({
        "ok": true,
        "plan_id": plan_id,
        "overall": if ok { "passed" } else { "blocked" },
        "gates": gates,
        "missing_required": missing_required,
        "failed_required": failed_required,
        "stale_required": stale_required,
        "unknown_required": unknown_required,
        "unsupported_required": unsupported_required,
    }))
}

fn gate_status_value(
    ctx: &RepoContext,
    plan_id: &str,
    gate: &WorkGateConfig,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> Result<Value> {
    match gate.kind.as_str() {
        "check" => {
            let tool_name = gate
                .tool
                .as_deref()
                .ok_or_else(|| anyhow!("Work gate '{}' is missing tool", gate.id))?;
            validate_check_tool(ctx, tool_name, "Work gate")?;
            let receipt = latest_plan_tool_receipt(ctx, plan_id, tool_name)?;
            let freshness_receipt = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => {
                    latest_plan_work_check_receipt_for_tool(
                        ctx,
                        plan_id,
                        tool_name,
                        &receipt.receipt_id,
                    )?
                    .or_else(|| Some(receipt.clone()))
                }
                _ => receipt.clone(),
            };
            let freshness = gate_freshness(&freshness_receipt, current_fingerprint);
            let status = match &receipt {
                Some(receipt) if receipt.exit_status == 0 => "passed",
                Some(_) => "failed",
                None => "missing",
            };
            let status = if status == "passed" && freshness != "fresh" {
                freshness
            } else {
                status
            };

            Ok(json!({
                "id": gate.id,
                "kind": gate.kind,
                "required": gate.required,
                "tool": tool_name,
                "status": status,
                "receipt_id": receipt.as_ref().map(|receipt| receipt.receipt_id.as_str()),
                "freshness_receipt_id": freshness_receipt
                    .as_ref()
                    .map(|receipt| receipt.receipt_id.as_str()),
                "exit_status": receipt.as_ref().map(|receipt| receipt.exit_status),
                "ended_at_ms": receipt.as_ref().map(|receipt| receipt.ended_at_ms),
                "freshness": freshness,
                "receipt_worktree_fingerprint_error": freshness_receipt
                    .as_ref()
                    .and_then(|receipt| receipt.worktree_fingerprint_error.as_deref()),
                "current_worktree_fingerprint_error": current_fingerprint.error.as_deref(),
            }))
        }
        other => Ok(json!({
            "id": gate.id,
            "kind": other,
            "required": gate.required,
            "status": "unsupported",
        })),
    }
}

fn collect_required_gate_failure(
    gate: &WorkGateConfig,
    status: &Value,
    missing_required: &mut Vec<String>,
    failed_required: &mut Vec<String>,
    stale_required: &mut Vec<String>,
    unknown_required: &mut Vec<String>,
    unsupported_required: &mut Vec<String>,
) {
    if !gate.required {
        return;
    }

    match status["status"].as_str() {
        Some("passed") => {}
        Some("missing") => missing_required.push(gate.id.clone()),
        Some("failed") => failed_required.push(gate.id.clone()),
        Some("stale") => stale_required.push(gate.id.clone()),
        Some("unknown") => unknown_required.push(gate.id.clone()),
        _ => unsupported_required.push(gate.id.clone()),
    }
}

fn ensure_required_gates_passed(ctx: &RepoContext, plan_id: &str) -> Result<()> {
    let status = gate_status(ctx, plan_id)?;
    if status["overall"] == "passed" {
        return Ok(());
    }

    let missing = gate_list(&status, "missing_required");
    let failed = gate_list(&status, "failed_required");
    let stale = gate_list(&status, "stale_required");
    let unknown = gate_list(&status, "unknown_required");
    let unsupported = gate_list(&status, "unsupported_required");
    let fingerprint_errors = gate_fingerprint_errors(&status);
    let fingerprint_error_details = if fingerprint_errors.is_empty() {
        String::new()
    } else {
        format!(" Fingerprint errors: [{}].", fingerprint_errors.join("; "))
    };

    bail!(
        "Required work gates are not satisfied for plan {plan_id}. Missing: [{}]. Failed: [{}]. Stale: [{}]. Unknown: [{}]. Unsupported: [{}].{} Run `scripts/jig work gates --plan-id {plan_id}` for details.",
        missing.join(", "),
        failed.join(", "),
        stale.join(", "),
        unknown.join(", "),
        unsupported.join(", "),
        fingerprint_error_details,
    )
}

fn gate_freshness(
    receipt: &Option<crate::state::ToolReceiptStatus>,
    current_fingerprint: &crate::state::CurrentWorktreeFingerprint,
) -> &'static str {
    let Some(receipt) = receipt else {
        return "missing";
    };
    let Some(receipt_fingerprint) = receipt.worktree_fingerprint.as_deref() else {
        return "unknown";
    };
    let Some(current_fingerprint) = current_fingerprint.fingerprint.as_deref() else {
        return "unknown";
    };
    if receipt_fingerprint == current_fingerprint {
        "fresh"
    } else {
        "stale"
    }
}

fn gate_list(status: &Value, key: &str) -> Vec<String> {
    status[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn gate_fingerprint_errors(status: &Value) -> Vec<String> {
    status["gates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|gate| {
            let id = gate["id"].as_str()?;
            let current = gate["current_worktree_fingerprint_error"].as_str();
            let receipt = gate["receipt_worktree_fingerprint_error"].as_str();
            match (current, receipt) {
                (None, None) => None,
                (Some(current), None) => Some(format!("{id}: current={}", concise_error(current))),
                (None, Some(receipt)) => Some(format!("{id}: receipt={}", concise_error(receipt))),
                (Some(current), Some(receipt)) => Some(format!(
                    "{id}: current={}, receipt={}",
                    concise_error(current),
                    concise_error(receipt)
                )),
            }
        })
        .collect()
}

fn concise_error(error: &str) -> String {
    let one_line = error.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_ERROR_CHARS: usize = 240;
    if one_line.chars().count() <= MAX_ERROR_CHARS {
        return one_line;
    }

    let mut truncated = one_line.chars().take(MAX_ERROR_CHARS).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn validate_check_tool(ctx: &RepoContext, name: &str, label: &str) -> Result<()> {
    let tool = ctx
        .tool_spec(name)
        .ok_or_else(|| anyhow!("{}", super::undeclared_tool_message(ctx, name)))?;
    if !tool_defs::is_execution_tool(tool) {
        bail!("{label} is not an execution tool: {name}");
    }
    if tool_defs::execution_tool_requires_name(tool) {
        bail!("{label} requires an argument and cannot run as a configured gate: {name}");
    }
    Ok(())
}
