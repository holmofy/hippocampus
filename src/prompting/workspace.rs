use std::path::Path;

use crate::policy::context::MAX_WORKSPACE_PROMPT_FILE_CHARS;

pub fn render_workspace_docs_section(workspace_root: &Path) -> String {
    let mut out = String::new();

    if let Some(block) = read_optional_workspace_file(workspace_root, "AGENTS.md") {
        out.push_str(&block);
        out.push_str("\n\n");
    }
    if let Some(block) = read_optional_workspace_file(workspace_root, "TOOLS.md") {
        out.push_str(&block);
        out.push_str("\n\n");
    }

    out.trim().to_string()
}

fn read_optional_workspace_file(workspace_root: &Path, filename: &str) -> Option<String> {
    let path = workspace_root.join(filename);
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let truncated = truncate_chars(trimmed, MAX_WORKSPACE_PROMPT_FILE_CHARS);
    let omitted = trimmed.chars().count() > MAX_WORKSPACE_PROMPT_FILE_CHARS;
    let mut block = format!("## Workspace: {filename}\n\n{truncated}");
    if omitted {
        block.push_str(&format!(
            "\n\n> 注意：该文件内容已截断到前 {MAX_WORKSPACE_PROMPT_FILE_CHARS} 个 Unicode 字符。"
        ));
    }
    Some(block)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}
