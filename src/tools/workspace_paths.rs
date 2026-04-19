use std::path::{Path, PathBuf};

use anyhow::Context;

/// 将 `user_path` 解析为 `workspace_root` 下的规范路径（禁止跳出工作区；解析符号链接后校验前缀）。
pub fn resolve_under_workspace(workspace_root: &Path, user_path: &str) -> anyhow::Result<PathBuf> {
    let ws = workspace_root
        .canonicalize()
        .with_context(|| format!("workspace_root not found: {}", workspace_root.display()))?;
    let p = Path::new(user_path.trim());
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        ws.join(p)
    };
    let cand = joined
        .canonicalize()
        .with_context(|| format!("path not found or inaccessible: {user_path}"))?;
    if !cand.starts_with(&ws) {
        anyhow::bail!("path escapes workspace_root");
    }
    Ok(cand)
}
