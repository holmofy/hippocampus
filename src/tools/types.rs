use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolCallId(pub String);

impl fmt::Display for ToolCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: String,
    /// ReAct 文本协议里 `Action Input:` 的原始字符串（通常是 JSON）。
    pub raw_input: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolErrorKind {
    Ok,
    UnknownTool,
    InvalidActionInputJson,
    JsonSchemaValidation,
    PolicyDenied,
    ToolExecution,
}

impl fmt::Display for ToolErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::UnknownTool => write!(f, "unknown_tool"),
            Self::InvalidActionInputJson => write!(f, "invalid_action_input_json"),
            Self::JsonSchemaValidation => write!(f, "json_schema_validation"),
            Self::PolicyDenied => write!(f, "policy_denied"),
            Self::ToolExecution => write!(f, "tool_execution"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub ok: bool,
    pub kind: ToolErrorKind,
    pub message: String,
    pub data: Option<Value>,
}

impl ToolResult {
    pub fn success(data: Value) -> Self {
        Self {
            ok: true,
            kind: ToolErrorKind::Ok,
            message: "ok".into(),
            data: Some(data),
        }
    }

    pub fn err(kind: ToolErrorKind, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            kind,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionRecord {
    pub call: ToolCall,
    pub result: ToolResult,
    pub started_at: Instant,
    pub finished_at: Instant,
    pub truncated: bool,
}

impl ToolExecutionRecord {
    pub fn duration(&self) -> Duration {
        self.finished_at.saturating_duration_since(self.started_at)
    }
}
