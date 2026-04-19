use std::path::PathBuf;
use std::sync::OnceLock;

use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::policy::ToolCategory;
use crate::tools::workspace_paths::resolve_under_workspace;
use crate::tools::Tool;

const MAX_MATCHES: usize = 120;
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// 在工作区内用正则搜索文件内容（递归；跳过过大/非 UTF-8 文件）。
pub struct GrepSearchTool {
    workspace_root: PathBuf,
}

impl GrepSearchTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl Tool for GrepSearchTool {
    fn name(&self) -> &'static str {
        "grep_search"
    }

    fn description(&self) -> &'static str {
        "Search UTF-8 text files under workspace with a Rust regex pattern (recursive)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[]
    }

    fn input_schema(&self) -> &Value {
        grep_search_schema()
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
        let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;

        let base_rel = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let base = resolve_under_workspace(&self.workspace_root, base_rel)?;
        if !base.is_dir() {
            anyhow::bail!("path must be a directory: {}", base.display());
        }

        let glob_filter: Option<GlobSet> = if let Some(g) = input.get("glob").and_then(|v| v.as_str()) {
            if g.trim().is_empty() {
                None
            } else {
                let mut b = GlobSetBuilder::new();
                b.add(Glob::new(g.trim()).map_err(|e| anyhow::anyhow!("invalid glob: {e}"))?);
                Some(b.build()?)
            }
        } else {
            None
        };

        let head_limit = input
            .get("head_limit")
            .and_then(|v| v.as_u64())
            .filter(|&n| n >= 1)
            .unwrap_or(MAX_MATCHES as u64) as usize;
        let head_limit = head_limit.min(MAX_MATCHES);

        let ws = self
            .workspace_root
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("workspace_root: {e}"))?;

        let mut matches = Vec::new();
        'outer: for ent in WalkDir::new(&base)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !ent.file_type().is_file() {
                continue;
            }
            let p = ent.path();
            let Ok(meta) = p.metadata() else {
                continue;
            };
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(can) = p.canonicalize() else {
                continue;
            };
            if !can.starts_with(&ws) {
                continue;
            }
            let rel = can.strip_prefix(&ws).unwrap_or(&can);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if let Some(gs) = &glob_filter {
                if !gs.is_match(&rel_s) {
                    continue;
                }
            }
            let Ok(text) = std::fs::read_to_string(&can) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    matches.push(json!({
                        "path": rel_s,
                        "line": i + 1,
                        "text": line,
                    }));
                    if matches.len() >= head_limit {
                        break 'outer;
                    }
                }
            }
        }
        let truncated = matches.len() >= head_limit;
        Ok(json!({
            "matches": matches,
            "truncated": truncated,
            "head_limit": head_limit,
        }))
    }
}

static GREP_SEARCH_SCHEMA: OnceLock<Value> = OnceLock::new();

fn grep_search_schema() -> &'static Value {
    GREP_SEARCH_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "pattern": { "type": "string", "minLength": 1 },
                "path": { "type": "string" },
                "glob": { "type": "string" },
                "head_limit": { "type": "integer", "minimum": 1, "maximum": 120 }
              },
              "required": ["pattern"]
            }"#,
        )
        .expect("grep_search schema")
    })
}
