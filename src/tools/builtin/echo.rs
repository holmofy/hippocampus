use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::policy::ToolCategory;
use crate::tools::Tool;

/// 示例工具：原样返回输入（支持“纯字符串”或 `{"text": "..."}` 两种输入形态）。
pub struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echo the provided text back to the model (for wiring tests)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[]
    }

    fn input_schema(&self) -> &Value {
        echo_input_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let text = match input {
            Value::String(s) => s,
            Value::Object(map) => map
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            _ => return Ok(json!({"error":"unsupported_input_shape"})),
        };
        Ok(json!({"text": text}))
    }
}

static ECHO_INPUT_SCHEMA: OnceLock<Value> = OnceLock::new();

fn echo_input_schema() -> &'static Value {
    ECHO_INPUT_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "oneOf": [
                {"type": "string"},
                {
                  "type": "object",
                  "additionalProperties": false,
                  "properties": {
                    "text": {"type": "string"}
                  },
                  "required": ["text"]
                }
              ]
            }"#,
        )
        .expect("echo input schema json")
    })
}
