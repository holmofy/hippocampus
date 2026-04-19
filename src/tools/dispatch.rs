use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use anyhow::Context;
use jsonschema::Validator;
use serde_json::Value;

use crate::policy::PromptContext;

use super::traits::Tool;
use super::types::{ToolCall, ToolErrorKind, ToolExecutionRecord, ToolResult};

/// Observation 文本上限（按 Unicode scalar 截断）。
pub const MAX_TOOL_OBSERVATION_CHARS: usize = 12_000;

struct SchemaCache {
    inner: Mutex<HashMap<String, Validator>>,
}

impl SchemaCache {
    fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn validate(&self, tool_name: &str, schema: &Value, instance: &Value) -> Result<(), String> {
        let mut map = self.inner.lock().map_err(|_| "schema_cache_poisoned".to_string())?;

        // Fast path.
        let compiled = match map.get(tool_name) {
            Some(s) => s,
            None => {
                let compiled = Validator::new(schema)
                    .map_err(|e| format!("schema compile failed for tool '{tool_name}': {e}"))?;
                map.insert(tool_name.to_string(), compiled);
                map.get(tool_name).expect("just inserted")
            }
        };

        compiled
            .validate(instance)
            .map_err(|e| format!("schema validation failed for tool '{tool_name}': {e}"))
    }

}

static SCHEMA_CACHE: OnceLock<SchemaCache> = OnceLock::new();

fn schema_cache() -> &'static SchemaCache {
    SCHEMA_CACHE.get_or_init(|| SchemaCache::new())
}

pub fn parse_action_input_json(raw: &str) -> anyhow::Result<Value> {
    let v: Value = serde_json::from_str(raw).with_context(|| "Action Input is not valid JSON")?;
    Ok(v)
}

pub fn execute_tool_call(
    tools: &[Box<dyn Tool>],
    prompt_ctx: &PromptContext,
    call: ToolCall,
) -> ToolExecutionRecord {
    let started_at = Instant::now();

    let Some(tool) = tools.iter().find(|t| t.name() == call.name) else {
        let finished_at = Instant::now();
        let name = call.name.clone();
        return ToolExecutionRecord {
            call,
            result: ToolResult::err(ToolErrorKind::UnknownTool, format!("unknown tool: {name}")),
            started_at,
            finished_at,
            truncated: false,
        };
    };

    if !prompt_ctx.is_tool_allowed_by_profile(tool.name()) {
        let finished_at = Instant::now();
        return ToolExecutionRecord {
            call,
            result: ToolResult::err(
                ToolErrorKind::PolicyDenied,
                format!(
                    "tool '{}' is not in the active profile allowlist (profile={})",
                    tool.name(),
                    prompt_ctx.tool_profile
                ),
            ),
            started_at,
            finished_at,
            truncated: false,
        };
    }

    for cap in tool.required_capabilities() {
        if !prompt_ctx.allows_category(*cap) {
            let finished_at = Instant::now();
            return ToolExecutionRecord {
                call,
                result: ToolResult::err(
                    ToolErrorKind::PolicyDenied,
                    format!("capability denied for tool '{}': {:?}", tool.name(), cap),
                ),
                started_at,
                finished_at,
                truncated: false,
            };
        }
    }

    let parsed = match parse_action_input_json(&call.raw_input) {
        Ok(v) => v,
        Err(e) => {
            let finished_at = Instant::now();
            return ToolExecutionRecord {
                call,
                result: ToolResult::err(ToolErrorKind::InvalidActionInputJson, format!("{e:#}")),
                started_at,
                finished_at,
                truncated: false,
            };
        }
    };

    if let Err(e) = schema_cache().validate(tool.name(), tool.input_schema(), &parsed) {
        let finished_at = Instant::now();
        return ToolExecutionRecord {
            call,
            result: ToolResult::err(ToolErrorKind::JsonSchemaValidation, e),
            started_at,
            finished_at,
            truncated: false,
        };
    }

    match tool.run_json(parsed) {
        Ok(data) => {
            let finished_at = Instant::now();
            ToolExecutionRecord {
                call,
                result: ToolResult::success(data),
                started_at,
                finished_at,
                truncated: false,
            }
        }
        Err(e) => {
            let finished_at = Instant::now();
            ToolExecutionRecord {
                call,
                result: ToolResult::err(ToolErrorKind::ToolExecution, format!("{e:#}")),
                started_at,
                finished_at,
                truncated: false,
            }
        }
    }
}

impl ToolExecutionRecord {
    pub fn observation_text(&self) -> String {
        let ms = self.duration().as_millis();
        let payload = serde_json::json!({
            "tool_call": {"id": self.call.id.0, "name": self.call.name},
            "ok": self.result.ok,
            "kind": self.result.kind.to_string(),
            "message": self.result.message,
            "data": self.result.data,
            "ms": ms,
        });
        let mut s = serde_json::to_string(&payload).unwrap_or_else(|_| "{\"ok\":false}".into());
        let truncated = truncate_chars_in_place(&mut s, MAX_TOOL_OBSERVATION_CHARS);
        let mut out = s;
        if truncated {
            out.push_str(" /* truncated */");
        }
        out
    }
}

fn truncate_chars_in_place(s: &mut String, max_chars: usize) -> bool {
    let n = s.chars().count();
    if n <= max_chars {
        return false;
    }
    *s = s.chars().take(max_chars).collect();
    true
}
