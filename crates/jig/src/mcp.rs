use std::io::{self, BufRead, Write};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};

use crate::context::{ManifestTool, RepoContext};
use crate::runtime::{call_tool, tool_specs};

pub fn serve(ctx: &RepoContext) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    loop {
        let Some(message) = read_message(&mut reader)? else {
            return Ok(());
        };

        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };

        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {
                        "tools": {
                            "listChanged": false
                        }
                    },
                    "serverInfo": {
                        "name": "jig",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            })),
            "notifications/initialized" => None,
            "ping" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            })),
            "tools/list" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": tool_specs(ctx).iter().map(tool_descriptor).collect::<Vec<_>>()
                }
            })),
            "tools/call" => Some(handle_tool_call(ctx, id, params)),
            other => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Unsupported method: {other}")
                }
            })),
        };

        if let Some(response) = response {
            write_message(&mut writer, &response)?;
        }
    }
}

fn handle_tool_call(ctx: &RepoContext, id: Option<Value>, params: Value) -> Value {
    let result = (|| -> Result<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tools/call requires params.name"))?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let tool_result = call_tool(ctx, name, args)?;
        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&tool_result)?
                }
            ],
            "structuredContent": tool_result,
            "isError": false
        }))
    })();

    match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }),
        Err(error) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32000,
                "message": error.to_string()
            }
        }),
    }
}

fn tool_descriptor(tool: &ManifestTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "inputSchema": input_schema(tool)
    })
}

fn input_schema(tool: &ManifestTool) -> Value {
    if tool.kind == "make" {
        return make_input_schema(tool);
    }

    match tool.name.as_str() {
        "jig.session_start" => empty_input_schema(),
        "jig.session_end" => json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "outcome": { "type": "string" }
            },
            "additionalProperties": false
        }),
        "jig.plans_open" => json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "body": { "type": "string" },
                "body_file": { "type": "string" }
            },
            "required": ["title"],
            "additionalProperties": false
        }),
        "jig.plans_append" => json!({
            "type": "object",
            "properties": {
                "plan_id": { "type": "string" },
                "body": { "type": "string" },
                "body_file": { "type": "string" }
            },
            "required": ["plan_id"],
            "additionalProperties": false
        }),
        "jig.plans_close" => json!({
            "type": "object",
            "properties": {
                "plan_id": { "type": "string" },
                "resolution": { "type": "string" }
            },
            "required": ["plan_id"],
            "additionalProperties": false
        }),
        "jig.receipts_list" => json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "plan_id": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1 }
            },
            "additionalProperties": false
        }),
        "jig.decisions_add" => json!({
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "selected_option": { "type": "string" },
                "rationale": { "type": "string" },
                "alternatives": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "plan_id": { "type": "string" }
            },
            "required": ["title", "selected_option", "rationale"],
            "additionalProperties": false
        }),
        _ => empty_input_schema(),
    }
}

fn make_input_schema(tool: &ManifestTool) -> Value {
    match tool.name.as_str() {
        "jig.migration_add" => named_make_input_schema(),
        _ if tool.target.is_none() => named_make_input_schema(),
        _ => plan_id_input_schema(),
    }
}

fn empty_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

fn plan_id_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "plan_id": { "type": "string" }
        },
        "additionalProperties": false
    })
}

fn named_make_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "plan_id": { "type": "string" }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn read_message(reader: &mut dyn BufRead) -> Result<Option<Value>> {
    let mut content_length = None::<usize>;
    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            return Ok(None);
        }

        if line == "\r\n" {
            break;
        }

        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let content_length = content_length.ok_or_else(|| anyhow!("Missing Content-Length header"))?;
    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;
    let message = serde_json::from_slice(&body).context("Failed to decode MCP message body")?;
    Ok(Some(message))
}

fn write_message(writer: &mut dyn Write, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}
