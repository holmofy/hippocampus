use std::fmt;

/// 预置工具配置（profile）：同一运行时通过不同白名单装配工具，便于审计与回放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProfile {
    /// 改仓库 / 编码：默认不放网络类工具进工具 belt（与 capabilities 正交）。
    Coding,
    /// 办公自动化向：允许网络类示例工具等（仍需 `capability_network` 才能真正执行）。
    Office,
    /// 当前内置工具全集（随 builtins 增长而扩展）。
    Full,
    /// 最小面：仅保留安全基线工具。
    Minimal,
    /// 由 `HIPPOCAMPUS_TOOL_ALLOWLIST` 显式列出（逗号分隔）。
    Custom,
}

impl ToolProfile {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "coding" | "code" => Ok(Self::Coding),
            "office" => Ok(Self::Office),
            "full" => Ok(Self::Full),
            "minimal" | "min" => Ok(Self::Minimal),
            "custom" => Ok(Self::Custom),
            other => Err(format!("unknown tool profile: {other}")),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::Office => "office",
            Self::Full => "full",
            Self::Minimal => "minimal",
            Self::Custom => "custom",
        }
    }
}

impl fmt::Display for ToolProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 将 profile 解析为**有序、去重**的工具名列表（用于 prompt 审计与 `assemble_tools`）。
pub fn resolve_tool_allowlist(
    profile: ToolProfile,
    custom_allowlist: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let mut names: Vec<String> = match profile {
        ToolProfile::Coding => vec![
            "echo".into(),
            "read_file".into(),
            "list_dir".into(),
            "glob_search".into(),
            "grep_search".into(),
            "lsp".into(),
        ],
        ToolProfile::Minimal => vec!["echo".into(), "read_file".into()],
        ToolProfile::Office => vec![
            "echo".into(),
            "read_file".into(),
            "list_dir".into(),
            "glob_search".into(),
            "grep_search".into(),
            "lsp".into(),
            "net_echo".into(),
        ],
        ToolProfile::Full => vec![
            "echo".into(),
            "glob_search".into(),
            "grep_search".into(),
            "list_dir".into(),
            "lsp".into(),
            "net_echo".into(),
            "read_file".into(),
        ],
        ToolProfile::Custom => {
            let raw = custom_allowlist
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "HIPPOCAMPUS_TOOL_ALLOWLIST is required when HIPPOCAMPUS_TOOL_PROFILE=custom"
                    )
                })?;
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        }
    };

    for n in &names {
        if !is_known_tool_name(n) {
            anyhow::bail!("unknown tool name in allowlist: {n}");
        }
    }

    names.sort();
    names.dedup();
    if names.is_empty() {
        anyhow::bail!("resolved tool allowlist is empty");
    }
    Ok(names)
}

pub(crate) fn is_known_tool_name(name: &str) -> bool {
    matches!(
        name,
        "echo" | "net_echo" | "read_file" | "list_dir" | "glob_search" | "grep_search" | "lsp"
    )
}
