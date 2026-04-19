use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::policy::ToolCategory;
use crate::tools::workspace_paths::resolve_under_workspace;
use crate::tools::Tool;

const MAX_ENTRIES: usize = 500;
const DEFAULT_LIMIT: u64 = 200;

/// 列出工作区内目录内容（不递归）。
pub struct ListDirTool {
    workspace_root: PathBuf,
}

impl ListDirTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl Tool for ListDirTool {
    fn name(&self) -> &'static str {
        "list_dir"
    }

    fn description(&self) -> &'static str {
        "List files and subdirectories in a workspace directory (non-recursive)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[]
    }

    fn input_schema(&self) -> &Value {
        list_dir_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let rel = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .filter(|&n| n >= 1)
            .unwrap_or(DEFAULT_LIMIT) as usize;
        let limit = limit.min(MAX_ENTRIES);

        let dir = resolve_under_workspace(&self.workspace_root, rel)?;
        if !dir.is_dir() {
            anyhow::bail!("not a directory: {}", dir.display());
        }
        let mut entries: Vec<Value> = Vec::new();
        for rd in std::fs::read_dir(&dir)? {
            let rd = rd?;
            let ft = rd.file_type()?;
            let name = rd.file_name().to_string_lossy().to_string();
            entries.push(json!({
                "name": name,
                "is_dir": ft.is_dir(),
                "is_file": ft.is_file(),
            }));
        }
        entries.sort_by(|a, b| {
            let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            na.cmp(nb)
        });
        let truncated = entries.len() > limit;
        entries.truncate(limit);
        Ok(json!({
            "path": dir.display().to_string(),
            "entries": entries,
            "truncated": truncated,
            "limit": limit,
        }))
    }
}

static LIST_DIR_SCHEMA: OnceLock<Value> = OnceLock::new();

fn list_dir_schema() -> &'static Value {
    LIST_DIR_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "path": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
              }
            }"#,
        )
        .expect("list_dir schema")
    })
}
