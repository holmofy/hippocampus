use std::path::PathBuf;
use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::policy::ToolCategory;
use crate::tools::workspace_paths::resolve_under_workspace;
use crate::tools::Tool;

const MAX_MATCHES: usize = 500;

/// 在工作区内按 glob 模式查找文件路径（递归）。
pub struct GlobSearchTool {
    workspace_root: PathBuf,
}

impl GlobSearchTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl Tool for GlobSearchTool {
    fn name(&self) -> &'static str {
        "glob_search"
    }

    fn description(&self) -> &'static str {
        "Find files under workspace_root matching a glob pattern (recursive walk)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[]
    }

    fn input_schema(&self) -> &Value {
        glob_search_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if pattern.is_empty() {
            anyhow::bail!("pattern is required");
        }
        let base_rel = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let base = resolve_under_workspace(&self.workspace_root, base_rel)?;
        if !base.is_dir() {
            anyhow::bail!("path must be a directory: {}", base.display());
        }

        let mut builder = GlobSetBuilder::new();
        builder.add(
            Glob::new(pattern).map_err(|e| anyhow::anyhow!("invalid glob: {e}"))?,
        );
        let set: GlobSet = builder.build()?;

        let ws = self
            .workspace_root
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("workspace_root: {e}"))?;

        let mut matches = Vec::new();
        for ent in WalkDir::new(&base)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !ent.file_type().is_file() {
                continue;
            }
            let p = ent.path();
            let Ok(can) = p.canonicalize() else {
                continue;
            };
            if !can.starts_with(&ws) {
                continue;
            }
            let rel = can.strip_prefix(&ws).unwrap_or(&can);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if set.is_match(&rel_s) {
                matches.push(json!(rel_s));
                if matches.len() >= MAX_MATCHES {
                    break;
                }
            }
        }
        let truncated = matches.len() >= MAX_MATCHES;
        Ok(json!({ "matches": matches, "truncated": truncated, "max": MAX_MATCHES }))
    }
}

static GLOB_SEARCH_SCHEMA: OnceLock<Value> = OnceLock::new();

fn glob_search_schema() -> &'static Value {
    GLOB_SEARCH_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "pattern": { "type": "string", "minLength": 1 },
                "path": { "type": "string" }
              },
              "required": ["pattern"]
            }"#,
        )
        .expect("glob_search schema")
    })
}
