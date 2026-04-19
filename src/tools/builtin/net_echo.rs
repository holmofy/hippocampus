use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::policy::ToolCategory;
use crate::tools::Tool;

/// 示例“网络类”工具：不会真的发起网络请求，但会要求 `PromptContext.capability_network=true` 才能执行。
pub struct NetworkEchoTool;

impl Tool for NetworkEchoTool {
    fn name(&self) -> &'static str {
        "net_echo"
    }

    fn description(&self) -> &'static str {
        "Pretend network tool (policy-gated). Returns a JSON echo of the requested URL."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[ToolCategory::Network]
    }

    fn input_schema(&self) -> &Value {
        net_echo_input_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(json!({"url": url, "status": "ok (simulated)"}))
    }
}

static NET_ECHO_INPUT_SCHEMA: OnceLock<Value> = OnceLock::new();

fn net_echo_input_schema() -> &'static Value {
    NET_ECHO_INPUT_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "url": {"type": "string", "minLength": 1}
              },
              "required": ["url"]
            }"#,
        )
        .expect("net_echo input schema json")
    })
}
