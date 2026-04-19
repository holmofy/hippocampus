/// 工具“危险等级/权限域”分类：用于执行期 capabilities 强制（不靠 prompt 自觉）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Network,
    Shell,
    WriteFile,
    /// 启动/对话语言服务器子进程（stdio LSP）；与「读文件」类能力正交。
    Lsp,
}
