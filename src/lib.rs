//! 最简单的 ReAct 风格循环：Thought → Action → Observation，直到 Final Answer。
//!
//! 该 crate 刻意保持“小而可扩展”：核心模块拆分为 `policy/`、`prompting/`、`tools/`、`react/`、`llm/`、`providers/`。

pub mod llm;
pub mod policy;
pub mod prompting;
pub mod providers;
pub mod react;
pub mod tools;

pub use llm::{LlmProvider, ProviderRegistry};
pub use policy::{PromptContext, ToolCategory, ToolProfile};
pub use providers::OpenAiCompatibleProvider;
pub use react::{ReactConfig, ReactError, ReactLoop};
pub use tools::{
    assemble_tools, EchoTool, GlobSearchTool, GrepSearchTool, ListDirTool, LspTool,
    NetworkEchoTool, ReadFileTool, Tool,
};

pub use policy::context::MAX_WORKSPACE_PROMPT_FILE_CHARS;

#[cfg(test)]
mod tests;
