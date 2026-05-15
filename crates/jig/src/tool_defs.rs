use anyhow::{Result, anyhow};
use serde_json::{Map, Value, json};

use crate::context::ManifestTool;

pub(crate) mod args {
    pub(crate) const ALTERNATIVES: &str = "alternatives";
    pub(crate) const BODY: &str = "body";
    pub(crate) const BODY_FILE: &str = "body_file";
    pub(crate) const FAILED_ONLY: &str = "failed_only";
    pub(crate) const LIMIT: &str = "limit";
    pub(crate) const NAME: &str = "name";
    pub(crate) const NOTES: &str = "notes";
    pub(crate) const OPERATION: &str = "operation";
    pub(crate) const OUTCOME: &str = "outcome";
    pub(crate) const PLAN_ID: &str = "plan_id";
    pub(crate) const RATIONALE: &str = "rationale";
    pub(crate) const RESOLUTION: &str = "resolution";
    pub(crate) const SELECTED_OPTION: &str = "selected_option";
    pub(crate) const SESSION_ID: &str = "session_id";
    pub(crate) const SUCCESS: &str = "success";
    pub(crate) const TITLE: &str = "title";
    pub(crate) const TOOL_NAME: &str = "tool_name";
    pub(crate) const TOOLS: &str = "tools";
    pub(crate) const CHECKPOINTS: &str = "checkpoints";
    pub(crate) const CONSTRAINTS: &str = "constraints";
    pub(crate) const OBJECTIVE: &str = "objective";
    pub(crate) const VALIDATIONS: &str = "validations";
}

pub(crate) mod cli_command {
    pub(crate) const ADOPT: &str = "adopt";
    pub(crate) const AGENT: &str = "agent";
    pub(crate) const AGENT_MAP: &str = "agent-map";
    pub(crate) const AGENT_MAP_CHECK: &str = "check";
    pub(crate) const AGENT_MAP_GENERATE: &str = "generate";
    pub(crate) const AGENT_BOOTSTRAP: &str = "bootstrap";
    pub(crate) const AGENT_DOCTOR: &str = "doctor";
    // Top-level `jig bootstrap` and nested `jig agent bootstrap` intentionally
    // share the same parser label in different Clap command scopes.
    pub(crate) const BOOTSTRAP: &str = "bootstrap";
    pub(crate) const CLIPPY: &str = "clippy";
    pub(crate) const CHECK_AGENT_GUIDES: &str = "check-agent-guides";
    pub(crate) const CHECK_MIGRATION_IMMUTABILITY: &str = "check-migration-immutability";
    pub(crate) const CHECK_NO_MOD_RS: &str = "check-no-mod-rs";
    pub(crate) const CHECK_RUST_FILE_LOC: &str = "check-rust-file-loc";
    pub(crate) const CHECK_SQLX_UNCHECKED_NON_TEST: &str = "check-sqlx-unchecked-non-test";
    pub(crate) const CONTRACT_CHECK: &str = "contract-check";
    pub(crate) const DEV: &str = "dev";
    pub(crate) const FMT_CHECK: &str = "fmt-check";
    pub(crate) const GENERATE_SQLX_UNCHECKED_QUERIES_TODO: &str =
        "generate-sqlx-unchecked-queries-todo";
    pub(crate) const INIT: &str = "init";
    pub(crate) const MCP: &str = "mcp";
    pub(crate) const MIGRATION_ADD: &str = "migration-add";
    pub(crate) const PROXY: &str = "proxy";
    pub(crate) const PROXY_ALIAS: &str = "alias";
    pub(crate) const PROXY_CERT: &str = "cert";
    pub(crate) const PROXY_CERT_GENERATE: &str = "generate";
    pub(crate) const PROXY_CERT_STATUS: &str = "status";
    pub(crate) const PROXY_CERT_TRUST: &str = "trust";
    pub(crate) const PROXY_CERT_UNTRUST: &str = "untrust";
    pub(crate) const PROXY_LIST: &str = "list";
    pub(crate) const PROXY_PRUNE: &str = "prune";
    pub(crate) const PROXY_RUN: &str = "run";
    pub(crate) const PROXY_SERVICE: &str = "service";
    pub(crate) const PROXY_SERVICE_INSTALL: &str = "install";
    pub(crate) const PROXY_SERVICE_STATUS: &str = "status";
    pub(crate) const PROXY_SERVICE_UNINSTALL: &str = "uninstall";
    pub(crate) const PROXY_START: &str = "start";
    pub(crate) const PROXY_STOP: &str = "stop";
    pub(crate) const RUN_TARGET: &str = "run-target";
    pub(crate) const SCHEMA_CHECK: &str = "schema-check";
    pub(crate) const SCHEMA_DUMP: &str = "schema-dump";
    pub(crate) const SQLX_CHECK: &str = "sqlx-check";
    pub(crate) const TEST: &str = "test";
    pub(crate) const TEST_LOCKED: &str = "test-locked";
    pub(crate) const UPDATE: &str = "update";
    pub(crate) const WORK: &str = "work";
    pub(crate) const WORK_APPEND: &str = "append";
    pub(crate) const WORK_CHECK: &str = "check";
    pub(crate) const WORK_DECIDE: &str = "decide";
    pub(crate) const WORK_FINISH: &str = "finish";
    pub(crate) const WORK_GATES: &str = "gates";
    pub(crate) const WORK_GOAL: &str = "goal";
    pub(crate) const WORK_RECEIPTS: &str = "receipts";
    pub(crate) const WORK_START: &str = "start";
    pub(crate) const WORK_STATUS: &str = "status";
}

pub(crate) mod kind {
    pub(crate) const COMMAND: &str = "command";
    pub(crate) const MAKE: &str = "make";
    pub(crate) const NATIVE: &str = "native";
}

pub(crate) mod tool {
    pub(crate) const BOOTSTRAP: &str = "jig.bootstrap";
    pub(crate) const AGENT_DOCTOR: &str = "jig.agent_doctor";
    pub(crate) const CLIPPY: &str = "jig.clippy";
    pub(crate) const CONTRACT_CHECK: &str = "jig.contract_check";
    pub(crate) const DECISIONS_ADD: &str = "jig.decisions_add";
    pub(crate) const FMT_CHECK: &str = "jig.fmt_check";
    pub(crate) const MIGRATION_ADD: &str = "jig.migration_add";
    pub(crate) const PLANS_APPEND: &str = "jig.plans_append";
    pub(crate) const PLANS_CLOSE: &str = "jig.plans_close";
    pub(crate) const PLANS_OPEN: &str = "jig.plans_open";
    pub(crate) const RUN_TARGET: &str = "jig.run_target";
    pub(crate) const SCHEMA_CHECK: &str = "jig.schema_check";
    pub(crate) const SCHEMA_DUMP: &str = "jig.schema_dump";
    pub(crate) const SESSION_END: &str = "jig.session_end";
    pub(crate) const SESSION_START: &str = "jig.session_start";
    pub(crate) const SQLX_CHECK: &str = "jig.sqlx_check";
    pub(crate) const TEST: &str = "jig.test";
    pub(crate) const TEST_LOCKED: &str = "jig.test_locked";
    pub(crate) const WORK_APPEND: &str = "jig.work_append";
    pub(crate) const WORK_CHECK: &str = "jig.work_check";
    pub(crate) const WORK_DECIDE: &str = "jig.work_decide";
    pub(crate) const WORK_FINISH: &str = "jig.work_finish";
    pub(crate) const WORK_GATES: &str = "jig.work_gates";
    pub(crate) const WORK_GOAL: &str = "jig.work_goal";
    pub(crate) const WORK_RECEIPTS: &str = "jig.work_receipts";
    pub(crate) const WORK_START: &str = "jig.work_start";
    pub(crate) const WORK_STATUS: &str = "jig.work_status";
}

pub(crate) type JsonObject = Map<String, Value>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryTool {
    AgentDoctor,
    Goal,
    Start,
    Append,
    Check,
    Gates,
    Decide,
    Receipts,
    Status,
    Finish,
}

impl MemoryTool {
    const ALL: [Self; 10] = [
        Self::AgentDoctor,
        Self::Goal,
        Self::Start,
        Self::Append,
        Self::Check,
        Self::Gates,
        Self::Decide,
        Self::Receipts,
        Self::Status,
        Self::Finish,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            tool::AGENT_DOCTOR => Some(Self::AgentDoctor),
            tool::WORK_GOAL => Some(Self::Goal),
            tool::WORK_START => Some(Self::Start),
            tool::WORK_APPEND => Some(Self::Append),
            tool::WORK_CHECK => Some(Self::Check),
            tool::WORK_GATES => Some(Self::Gates),
            tool::WORK_DECIDE => Some(Self::Decide),
            tool::WORK_RECEIPTS => Some(Self::Receipts),
            tool::WORK_STATUS => Some(Self::Status),
            tool::WORK_FINISH => Some(Self::Finish),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::AgentDoctor => tool::AGENT_DOCTOR,
            Self::Goal => tool::WORK_GOAL,
            Self::Start => tool::WORK_START,
            Self::Append => tool::WORK_APPEND,
            Self::Check => tool::WORK_CHECK,
            Self::Gates => tool::WORK_GATES,
            Self::Decide => tool::WORK_DECIDE,
            Self::Receipts => tool::WORK_RECEIPTS,
            Self::Status => tool::WORK_STATUS,
            Self::Finish => tool::WORK_FINISH,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::AgentDoctor => "Report local Codex agent tooling status for this repo.",
            Self::Goal => {
                "Create a goal-mode work harness with a durable plan and validation contract."
            }
            Self::Start => "Start structured work by opening a session and plan.",
            Self::Append => "Append to a structured work plan.",
            Self::Check => "Run configured or selected work checks.",
            Self::Gates => "Report configured work gate status for a plan.",
            Self::Decide => "Record a structured work decision.",
            Self::Receipts => "List structured work receipts.",
            Self::Status => "Summarize structured work state.",
            Self::Finish => "Close a structured work plan and active session.",
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::AgentDoctor | Self::Status => empty_input_schema(),
            Self::Goal => object_schema(
                &[
                    (args::OBJECTIVE, string_schema()),
                    (args::SUCCESS, string_schema()),
                    (
                        args::VALIDATIONS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        }),
                    ),
                    (
                        args::CONSTRAINTS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (
                        args::CHECKPOINTS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (args::TITLE, string_schema()),
                    (args::NOTES, string_schema()),
                ],
                &[args::OBJECTIVE, args::SUCCESS, args::VALIDATIONS],
            ),
            Self::Gates => object_schema(&[(args::PLAN_ID, string_schema())], &[args::PLAN_ID]),
            Self::Start => object_schema(
                &[
                    (args::TITLE, string_schema()),
                    (args::BODY, string_schema()),
                    (args::BODY_FILE, string_schema()),
                ],
                &[args::TITLE],
            ),
            Self::Append => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (args::BODY, string_schema()),
                    (args::BODY_FILE, string_schema()),
                ],
                &[args::PLAN_ID],
            ),
            Self::Check => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (
                        args::TOOLS,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                ],
                &[args::PLAN_ID],
            ),
            Self::Decide => object_schema(
                &[
                    (args::TITLE, string_schema()),
                    (args::SELECTED_OPTION, string_schema()),
                    (args::RATIONALE, string_schema()),
                    (
                        args::ALTERNATIVES,
                        json!({
                            "type": "array",
                            "items": { "type": "string" }
                        }),
                    ),
                    (args::PLAN_ID, string_schema()),
                ],
                &[args::TITLE, args::SELECTED_OPTION, args::RATIONALE],
            ),
            Self::Receipts => object_schema(
                &[
                    (args::SESSION_ID, string_schema()),
                    (args::PLAN_ID, string_schema()),
                    (args::TOOL_NAME, string_schema()),
                    (args::FAILED_ONLY, json!({ "type": "boolean" })),
                    (args::LIMIT, json!({ "type": "integer", "minimum": 1 })),
                ],
                &[],
            ),
            Self::Finish => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (args::RESOLUTION, string_schema()),
                    (args::OUTCOME, string_schema()),
                ],
                &[args::PLAN_ID],
            ),
        }
    }
}

pub(crate) fn tool_descriptors(manifest_tools: &[ManifestTool]) -> Vec<Value> {
    manifest_tools
        .iter()
        .filter(|tool| is_execution_tool(tool))
        .map(manifest_tool_descriptor)
        .chain(MemoryTool::ALL.into_iter().map(memory_tool_descriptor))
        .collect()
}

fn manifest_tool_descriptor(tool: &ManifestTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": execution_input_schema(tool)
    })
}

fn memory_tool_descriptor(tool: MemoryTool) -> Value {
    json!({
        "name": tool.name(),
        "description": tool.description(),
        "inputSchema": tool.input_schema()
    })
}

pub(crate) fn is_make_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::MAKE
}

pub(crate) fn is_command_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::COMMAND
}

pub(crate) fn is_native_tool(tool: &ManifestTool) -> bool {
    tool.kind == kind::NATIVE
}

pub(crate) fn is_execution_tool(tool: &ManifestTool) -> bool {
    is_make_tool(tool) || is_command_tool(tool) || is_native_tool(tool)
}

pub(crate) fn execution_tool_args(tool: &ManifestTool, args_obj: &JsonObject) -> Result<Value> {
    if execution_tool_requires_name(tool) {
        let name = required_string_arg(args_obj, args::NAME)?;
        return Ok(object_value([(args::NAME, Value::String(name))]));
    }

    Ok(json!({}))
}

pub(crate) fn execution_tool_requires_name(tool: &ManifestTool) -> bool {
    tool.name == tool::MIGRATION_ADD || (is_make_tool(tool) && tool.target.is_none())
}

fn execution_input_schema(tool: &ManifestTool) -> Value {
    if execution_tool_requires_name(tool) {
        return object_schema(
            &[
                (args::NAME, string_schema()),
                (args::PLAN_ID, string_schema()),
            ],
            &[args::NAME],
        );
    }

    object_schema(&[(args::PLAN_ID, string_schema())], &[])
}

fn empty_input_schema() -> Value {
    object_schema(&[], &[])
}

fn object_schema(properties: &[(&str, Value)], required: &[&str]) -> Value {
    let mut schema = JsonObject::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert(
        "properties".into(),
        object_value(properties.iter().cloned()),
    );
    if !required.is_empty() {
        schema.insert(
            "required".into(),
            Value::Array(
                required
                    .iter()
                    .map(|required| Value::String((*required).into()))
                    .collect(),
            ),
        );
    }
    schema.insert("additionalProperties".into(), Value::Bool(false));
    Value::Object(schema)
}

fn object_value<'a>(entries: impl IntoIterator<Item = (&'a str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn string_schema() -> Value {
    json!({ "type": "string" })
}

pub(crate) fn required_string_arg(map: &JsonObject, key: &str) -> Result<String> {
    string_arg(map, key).ok_or_else(|| anyhow!("Missing required argument: {key}"))
}

pub(crate) fn string_arg(map: &JsonObject, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}
