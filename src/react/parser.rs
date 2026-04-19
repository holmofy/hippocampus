use std::sync::atomic::{AtomicU64, Ordering};

use crate::tools::{ToolCall, ToolCallId};

static CALL_ID_SEQ: AtomicU64 = AtomicU64::new(1);

/// 取出模型本轮输出里的 `Thought:` 段落（直到 `Action:` / `Final Answer:` 之前）。
pub fn parse_thought(text: &str) -> Option<String> {
    let key = "Thought:";
    let start = text.find(key)? + key.len();
    let tail = text.get(start..)?;
    let tail = tail.trim_start();
    let cut = tail
        .find("\nAction:")
        .or_else(|| tail.find("\r\nAction:"))
        .or_else(|| tail.find("\nFinal Answer:"))
        .or_else(|| tail.find("\r\nFinal Answer:"))
        .unwrap_or(tail.len());
    let s = tail.get(..cut)?.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

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
