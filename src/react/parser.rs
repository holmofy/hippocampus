use std::sync::atomic::{AtomicU64, Ordering};

use crate::tools::{ToolCall, ToolCallId};

static CALL_ID_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn parse_final_answer(text: &str) -> Option<&str> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Final Answer:") {
            return Some(rest);
        }
    }
    None
}

/// 从模型输出解析 ReAct 工具调用（兼容可选 `Call ID:`）。
pub fn parse_react_tool_call(text: &str) -> Option<ToolCall> {
    let mut action: Option<String> = None;
    let mut action_input: Option<String> = None;
    let mut call_id: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Action:") {
            action = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Action Input:") {
            action_input = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Call ID:") {
            call_id = Some(rest.trim().to_string());
        }
    }

    let name = action?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let raw_input = action_input?;

    let id = match call_id.filter(|s| !s.trim().is_empty()) {
        Some(s) => ToolCallId(s),
        None => ToolCallId(format!(
            "tc_{}",
            CALL_ID_SEQ.fetch_add(1, Ordering::Relaxed)
        )),
    };

    Some(ToolCall {
        id,
        name,
        raw_input,
    })
}
