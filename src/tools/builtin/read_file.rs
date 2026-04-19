use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::policy::ToolCategory;
use crate::tools::workspace_paths::resolve_under_workspace;
use crate::tools::Tool;

const MAX_READ_BYTES: usize = 512 * 1024;
const MAX_LINE_LIMIT: u64 = 2000;
const DEFAULT_LINE_LIMIT: u64 = 200;

/// 读取工作区内文本文件（按行 offset/limit，1-based 行号）。
pub struct ReadFileTool {
    workspace_root: PathBuf,
}

impl ReadFileTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl Tool for ReadFileTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read a UTF-8 text file under workspace_root (line slice via offset/limit)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[]
    }

    fn input_schema(&self) -> &Value {
        read_file_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if path.trim().is_empty() {
            anyhow::bail!("path is required");
        }
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .filter(|&n| n >= 1)
            .unwrap_or(1);
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .filter(|&n| n >= 1)
            .unwrap_or(DEFAULT_LINE_LIMIT)
            .min(MAX_LINE_LIMIT);

        let full = resolve_under_workspace(&self.workspace_root, &path)?;
        let raw = std::fs::read_to_string(&full)?;
        if raw.len() > MAX_READ_BYTES {
            anyhow::bail!("file too large (max {MAX_READ_BYTES} bytes)");
        }
        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();
        let total_lines = total as u64;
        let start_idx = offset.saturating_sub(1) as usize;
        let start_idx = start_idx.min(total);
        let lim = limit.min(MAX_LINE_LIMIT) as usize;
        let end_idx = start_idx.saturating_add(lim).min(total);
        let slice: Vec<String> = lines[start_idx..end_idx]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        Ok(json!({
            "path": full.display().to_string(),
            "offset": offset,
            "limit": limit,
            "total_lines": total_lines,
            "lines": slice,
        }))
    }
}

static READ_FILE_SCHEMA: OnceLock<Value> = OnceLock::new();

fn read_file_schema() -> &'static Value {
    READ_FILE_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "path": { "type": "string", "minLength": 1 },
                "offset": { "type": "integer", "minimum": 1 },
                "limit": { "type": "integer", "minimum": 1, "maximum": 2000 }
              },
              "required": ["path"]
            }"#,
        )
        .expect("read_file schema")
    })
}
