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
    pub(crate) const OUTCOME: &str = "outcome";
    pub(crate) const PLAN_ID: &str = "plan_id";
    pub(crate) const RATIONALE: &str = "rationale";
    pub(crate) const RESOLUTION: &str = "resolution";
    pub(crate) const SELECTED_OPTION: &str = "selected_option";
    pub(crate) const SESSION_ID: &str = "session_id";
    pub(crate) const TITLE: &str = "title";
    pub(crate) const TOOL_NAME: &str = "tool_name";
}

pub(crate) mod cli_command {
    pub(crate) const ADOPT: &str = "adopt";
    pub(crate) const CLIPPY: &str = "clippy";
    pub(crate) const CONTRACT_CHECK: &str = "contract-check";
    pub(crate) const DECISIONS_ADD: &str = "decisions-add";
    pub(crate) const FMT_CHECK: &str = "fmt-check";
    pub(crate) const INIT: &str = "init";
    pub(crate) const MCP: &str = "mcp";
    pub(crate) const MIGRATION_ADD: &str = "migration-add";
    pub(crate) const PLANS_APPEND: &str = "plans-append";
    pub(crate) const PLANS_CLOSE: &str = "plans-close";
    pub(crate) const PLANS_OPEN: &str = "plans-open";
    pub(crate) const RECEIPTS_LIST: &str = "receipts-list";
    pub(crate) const RUN_TARGET: &str = "run-target";
    pub(crate) const SCHEMA_CHECK: &str = "schema-check";
    pub(crate) const SCHEMA_DUMP: &str = "schema-dump";
    pub(crate) const SESSION_END: &str = "session-end";
    pub(crate) const SESSION_START: &str = "session-start";
    pub(crate) const SQLX_CHECK: &str = "sqlx-check";
    pub(crate) const STATE_SUMMARY: &str = "state-summary";
    pub(crate) const TEST: &str = "test";
    pub(crate) const TEST_LOCKED: &str = "test-locked";
    pub(crate) const UPDATE: &str = "update";
}

pub(crate) mod kind {
    pub(crate) const MAKE: &str = "make";
}

pub(crate) mod tool {
    pub(crate) const CLIPPY: &str = "jig.clippy";
    pub(crate) const CONTRACT_CHECK: &str = "jig.contract_check";
    pub(crate) const DECISIONS_ADD: &str = "jig.decisions_add";
    pub(crate) const FMT_CHECK: &str = "jig.fmt_check";
    pub(crate) const MIGRATION_ADD: &str = "jig.migration_add";
    pub(crate) const PLANS_APPEND: &str = "jig.plans_append";
    pub(crate) const PLANS_CLOSE: &str = "jig.plans_close";
    pub(crate) const PLANS_OPEN: &str = "jig.plans_open";
    pub(crate) const RECEIPTS_LIST: &str = "jig.receipts_list";
    pub(crate) const RUN_TARGET: &str = "jig.run_target";
    pub(crate) const SCHEMA_CHECK: &str = "jig.schema_check";
    pub(crate) const SCHEMA_DUMP: &str = "jig.schema_dump";
    pub(crate) const SESSION_END: &str = "jig.session_end";
    pub(crate) const SESSION_START: &str = "jig.session_start";
    pub(crate) const SQLX_CHECK: &str = "jig.sqlx_check";
    pub(crate) const STATE_SUMMARY: &str = "jig.state_summary";
    pub(crate) const TEST: &str = "jig.test";
    pub(crate) const TEST_LOCKED: &str = "jig.test_locked";
}

pub(crate) type JsonObject = Map<String, Value>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemoryTool {
    SessionStart,
    SessionEnd,
    PlansOpen,
    PlansAppend,
    PlansClose,
    ReceiptsList,
    StateSummary,
    DecisionsAdd,
}

impl MemoryTool {
    const ALL: [Self; 8] = [
        Self::SessionStart,
        Self::SessionEnd,
        Self::PlansOpen,
        Self::PlansAppend,
        Self::PlansClose,
        Self::ReceiptsList,
        Self::StateSummary,
        Self::DecisionsAdd,
    ];

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            tool::SESSION_START => Some(Self::SessionStart),
            tool::SESSION_END => Some(Self::SessionEnd),
            tool::PLANS_OPEN => Some(Self::PlansOpen),
            tool::PLANS_APPEND => Some(Self::PlansAppend),
            tool::PLANS_CLOSE => Some(Self::PlansClose),
            tool::RECEIPTS_LIST => Some(Self::ReceiptsList),
            tool::STATE_SUMMARY => Some(Self::StateSummary),
            tool::DECISIONS_ADD => Some(Self::DecisionsAdd),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SessionStart => tool::SESSION_START,
            Self::SessionEnd => tool::SESSION_END,
            Self::PlansOpen => tool::PLANS_OPEN,
            Self::PlansAppend => tool::PLANS_APPEND,
            Self::PlansClose => tool::PLANS_CLOSE,
            Self::ReceiptsList => tool::RECEIPTS_LIST,
            Self::StateSummary => tool::STATE_SUMMARY,
            Self::DecisionsAdd => tool::DECISIONS_ADD,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::SessionStart => "Open a new jig session and return recent repo context.",
            Self::SessionEnd => "Close a jig session.",
            Self::PlansOpen => "Open a structured plan and create its Markdown body.",
            Self::PlansAppend => "Append to a structured plan body.",
            Self::PlansClose => "Close a structured plan.",
            Self::ReceiptsList => "List structured receipts.",
            Self::StateSummary => "Summarize structured jig state.",
            Self::DecisionsAdd => "Append a structured decision record.",
        }
    }

    fn input_schema(self) -> Value {
        match self {
            Self::SessionStart | Self::StateSummary => empty_input_schema(),
            Self::SessionEnd => object_schema(
                &[
                    (args::SESSION_ID, string_schema()),
                    (args::OUTCOME, string_schema()),
                ],
                &[],
            ),
            Self::PlansOpen => object_schema(
                &[
                    (args::TITLE, string_schema()),
                    (args::BODY, string_schema()),
                    (args::BODY_FILE, string_schema()),
                ],
                &[args::TITLE],
            ),
            Self::PlansAppend => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (args::BODY, string_schema()),
                    (args::BODY_FILE, string_schema()),
                ],
                &[args::PLAN_ID],
            ),
            Self::PlansClose => object_schema(
                &[
                    (args::PLAN_ID, string_schema()),
                    (args::RESOLUTION, string_schema()),
                ],
                &[args::PLAN_ID],
            ),
            Self::ReceiptsList => object_schema(
                &[
                    (args::SESSION_ID, string_schema()),
                    (args::PLAN_ID, string_schema()),
                    (args::TOOL_NAME, string_schema()),
                    (args::FAILED_ONLY, json!({ "type": "boolean" })),
                    (args::LIMIT, json!({ "type": "integer", "minimum": 1 })),
                ],
                &[],
            ),
            Self::DecisionsAdd => object_schema(
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
        }
    }
}

pub(crate) fn tool_descriptors(manifest_tools: &[ManifestTool]) -> Vec<Value> {
    manifest_tools
        .iter()
        .filter(|tool| is_make_tool(tool))
        .map(manifest_tool_descriptor)
        .chain(MemoryTool::ALL.into_iter().map(memory_tool_descriptor))
        .collect()
}

fn manifest_tool_descriptor(tool: &ManifestTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": make_input_schema(tool)
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

pub(crate) fn make_tool_args(tool: &ManifestTool, args_obj: &JsonObject) -> Result<Value> {
    if make_tool_requires_name(tool) {
        let name = required_string_arg(args_obj, args::NAME)?;
        return Ok(object_value([(args::NAME, Value::String(name))]));
    }

    Ok(json!({}))
}

pub(crate) fn make_tool_requires_name(tool: &ManifestTool) -> bool {
    tool.name == tool::MIGRATION_ADD || tool.target.is_none()
}

fn make_input_schema(tool: &ManifestTool) -> Value {
    if make_tool_requires_name(tool) {
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

pub(crate) fn usize_arg(map: &JsonObject, key: &str) -> Option<usize> {
    map.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

pub(crate) fn bool_arg(map: &JsonObject, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

pub(crate) fn string_list_arg(map: &JsonObject, key: &str) -> Vec<String> {
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
