use crate::policy::PromptContext;

use super::builtin::{
    EchoTool, GlobSearchTool, GrepSearchTool, ListDirTool, LspTool, NetworkEchoTool, ReadFileTool,
};
use super::Tool;

/// 按 `PromptContext` 中的 profile / `allowed_tool_names` 装配工具实例（单一入口，便于审计）。
pub fn assemble_tools(ctx: &PromptContext) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let ws = ctx.workspace_root.clone();
    let mut out: Vec<Box<dyn Tool>> = Vec::new();
    for name in &ctx.allowed_tool_names {
        let tool: Box<dyn Tool> = match name.as_str() {
            "echo" => Box::new(EchoTool),
            "net_echo" => Box::new(NetworkEchoTool),
            "read_file" => Box::new(ReadFileTool::new(ws.clone())),
            "list_dir" => Box::new(ListDirTool::new(ws.clone())),
            "glob_search" => Box::new(GlobSearchTool::new(ws.clone())),
            "grep_search" => Box::new(GrepSearchTool::new(ws.clone())),
            "lsp" => Box::new(LspTool::new(ws.clone())),
            other => anyhow::bail!("unknown tool in PromptContext.allowed_tool_names: {other}"),
        };
        out.push(tool);
    }
    Ok(out)
}
