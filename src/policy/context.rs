use std::path::PathBuf;

use super::tool_profile::{resolve_tool_allowlist, ToolProfile};
use super::ToolCategory;

/// 工作区文档注入上限（单文件字符数，按 Unicode scalar 截断）。
pub const MAX_WORKSPACE_PROMPT_FILE_CHARS: usize = 20_000;

/// 组装到模型 prompt 里的“运行上下文”（偏 harness 契约：可审计/可回放）。
#[derive(Debug, Clone)]
pub struct PromptContext {
    /// 可选：用于追踪/回放（没有就显示为 none）。
    pub run_id: Option<String>,
    /// 工作区根目录：用于读取 `AGENTS.md` / `TOOLS.md` 等（也用于提示模型路径边界）。
    pub workspace_root: PathBuf,
    /// 工具 profile（与 capabilities 正交）：决定本 run **装配哪些工具**。
    pub tool_profile: ToolProfile,
    /// 由 profile 解析得到的允许工具名（有序、去重），写入 prompt 供审计。
    pub allowed_tool_names: Vec<String>,
    /// 能力开关（当前 hippocampus 示例只有 `echo`，但字段先对齐 harness 思维）。
    pub capability_network: bool,
    pub capability_shell: bool,
    pub capability_write_file: bool,
    pub capability_lsp: bool,
    pub max_tool_iterations: usize,
}

impl Default for PromptContext {
    fn default() -> Self {
        let allowed_tool_names =
            resolve_tool_allowlist(ToolProfile::Coding, None).expect("coding profile static");
        Self {
            run_id: None,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            tool_profile: ToolProfile::Coding,
            allowed_tool_names,
            capability_network: false,
            capability_shell: false,
            capability_write_file: false,
            capability_lsp: false,
            max_tool_iterations: 0,
        }
    }
}

impl PromptContext {
    /// 从环境变量读取（找不到就用 cwd），便于本地/CI 统一注入。
    ///
    /// 支持的环境变量（全部可选）：
    /// - `HIPPOCAMPUS_RUN_ID`
    /// - `HIPPOCAMPUS_WORKSPACE_ROOT`
    /// - `HIPPOCAMPUS_TOOL_PROFILE`：`coding` / `office` / `full` / `minimal` / `custom`（默认 `coding`）
    /// - `HIPPOCAMPUS_TOOL_ALLOWLIST`：`custom` 时必填，逗号分隔工具名（如 `echo,net_echo`）
    /// - `HIPPOCAMPUS_CAPABILITY_NETWORK` / `HIPPOCAMPUS_CAPABILITY_SHELL` / `HIPPOCAMPUS_CAPABILITY_WRITE_FILE` / `HIPPOCAMPUS_CAPABILITY_LSP`：`1/true/yes/on`
    pub fn from_env() -> anyhow::Result<Self> {
        let run_id = std::env::var("HIPPOCAMPUS_RUN_ID").ok().filter(|s| !s.trim().is_empty());

        let workspace_root = match std::env::var("HIPPOCAMPUS_WORKSPACE_ROOT") {
            Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => std::env::current_dir()?,
        };

        let tool_profile = match std::env::var("HIPPOCAMPUS_TOOL_PROFILE") {
            Ok(s) if !s.trim().is_empty() => ToolProfile::parse(&s).map_err(anyhow::Error::msg)?,
            _ => ToolProfile::Coding,
        };
        let custom_allowlist = std::env::var("HIPPOCAMPUS_TOOL_ALLOWLIST").ok();
        let allowed_tool_names = resolve_tool_allowlist(
            tool_profile,
            custom_allowlist.as_deref(),
        )?;

        fn parse_bool_flag(raw: Result<String, std::env::VarError>) -> bool {
            let Ok(v) = raw else {
                return false;
            };
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "y" | "on"
            )
        }

        Ok(Self {
            run_id,
            workspace_root,
            tool_profile,
            allowed_tool_names,
            capability_network: parse_bool_flag(std::env::var("HIPPOCAMPUS_CAPABILITY_NETWORK")),
            capability_shell: parse_bool_flag(std::env::var("HIPPOCAMPUS_CAPABILITY_SHELL")),
            capability_write_file: parse_bool_flag(std::env::var(
                "HIPPOCAMPUS_CAPABILITY_WRITE_FILE",
            )),
            capability_lsp: parse_bool_flag(std::env::var("HIPPOCAMPUS_CAPABILITY_LSP")),
            max_tool_iterations: 0,
        })
    }

    /// 运行时切换 profile（例如 CLI 覆盖），会重新读取 `HIPPOCAMPUS_TOOL_ALLOWLIST`。
    pub fn set_tool_profile(&mut self, profile: ToolProfile) -> anyhow::Result<()> {
        let custom_allowlist = std::env::var("HIPPOCAMPUS_TOOL_ALLOWLIST").ok();
        self.tool_profile = profile;
        self.allowed_tool_names = resolve_tool_allowlist(profile, custom_allowlist.as_deref())?;
        Ok(())
    }

    /// 第二层：profile 白名单（`allowed_tool_names` 为空时不做额外限制，便于单测构造旧上下文）。
    pub(crate) fn is_tool_allowed_by_profile(&self, tool_name: &str) -> bool {
        self.allowed_tool_names.is_empty()
            || self.allowed_tool_names.iter().any(|n| n == tool_name)
    }

    pub(crate) fn allows_category(&self, category: ToolCategory) -> bool {
        match category {
            ToolCategory::Network => self.capability_network,
            ToolCategory::Shell => self.capability_shell,
            ToolCategory::WriteFile => self.capability_write_file,
            ToolCategory::Lsp => self.capability_lsp,
        }
    }
}
