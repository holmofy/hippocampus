use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::Value;

use crate::policy::ToolCategory;
use crate::tools::lsp_stdio::{self, LspOperation};
use crate::tools::workspace_paths::resolve_under_workspace;
use crate::tools::Tool;

/// 对齐 OpenCode：`operation` + `path` + `line` + `character`（**1-based**，与编辑器展示一致）。
///
/// 语言服务器命令来自 `HIPPOCAMPUS_LSP_COMMAND`（默认 `rust-analyzer`），需自行安装并在 PATH 中可用。
pub struct LspTool {
    workspace_root: PathBuf,
    server_command: String,
}

impl LspTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        let server_command =
            std::env::var("HIPPOCAMPUS_LSP_COMMAND").unwrap_or_else(|_| "rust-analyzer".into());
        Self {
            workspace_root,
            server_command,
        }
    }
}

impl Tool for LspTool {
    fn name(&self) -> &'static str {
        "lsp"
    }

    fn description(&self) -> &'static str {
        "Language Server Protocol (stdio): go_to_definition | find_references at path:line:character (1-based). Requires HIPPOCAMPUS_CAPABILITY_LSP and a working LSP binary (default rust-analyzer)."
    }

    fn required_capabilities(&self) -> &'static [ToolCategory] {
        &[ToolCategory::Lsp]
    }

    fn input_schema(&self) -> &Value {
        lsp_input_schema()
    }

    fn run_json(&self, input: Value) -> anyhow::Result<Value> {
        let op_raw = input
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let op = LspOperation::parse(op_raw)?;
        let rel = input
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if rel.is_empty() {
            anyhow::bail!("path is required");
        }
        let line = input
            .get("line")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 1)
            .ok_or_else(|| anyhow::anyhow!("line must be a 1-based integer >= 1"))?;
        let character = input
            .get("character")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n >= 1)
            .ok_or_else(|| anyhow::anyhow!("character must be a 1-based integer >= 1"))?;

        let file = resolve_under_workspace(&self.workspace_root, rel)?;
        lsp_stdio::lsp_query_one_shot(
            &self.workspace_root,
            &self.server_command,
            op,
            &file,
            line,
            character,
        )
    }
}

static LSP_INPUT_SCHEMA: OnceLock<Value> = OnceLock::new();

fn lsp_input_schema() -> &'static Value {
    LSP_INPUT_SCHEMA.get_or_init(|| {
        serde_json::from_str(
            r#"{
              "type": "object",
              "additionalProperties": false,
              "properties": {
                "operation": {
                  "type": "string",
                  "enum": ["go_to_definition", "goToDefinition", "find_references", "findReferences"]
                },
                "path": { "type": "string", "minLength": 1 },
                "line": { "type": "integer", "minimum": 1 },
                "character": { "type": "integer", "minimum": 1 }
              },
              "required": ["operation", "path", "line", "character"]
            }"#,
        )
        .expect("lsp schema")
    })
}
