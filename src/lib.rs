//! 最简单的 ReAct 风格循环：Thought → Action → Observation，直到 Final Answer。
//!
//! 约定（可自行改格式/解析）：
//! - `Action: <tool_name>`
//! - `Action Input: <传给工具的字符串>`
//! - `Final Answer: <给用户的答案>`

use async_trait::async_trait;
use std::fmt;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PROMPT_CORE: &str = include_str!("prompt/core.md");
const PROMPT_REACT: &str = include_str!("prompt/react_format.md");
const PROMPT_SAFETY: &str = include_str!("prompt/safety.md");

/// 工作区文档注入上限（单文件字符数，按 Unicode scalar 截断）。
pub const MAX_WORKSPACE_PROMPT_FILE_CHARS: usize = 20_000;

/// 组装到模型 prompt 里的“运行上下文”（偏 harness 契约：可审计/可回放）。
#[derive(Debug, Clone)]
pub struct PromptContext {
    /// 可选：用于追踪/回放（没有就显示为 none）。
    pub run_id: Option<String>,
    /// 工作区根目录：用于读取 `AGENTS.md` / `TOOLS.md` 等（也用于提示模型路径边界）。
    pub workspace_root: PathBuf,
    /// 能力开关（当前 hippocampus 示例只有 `echo`，但字段先对齐 harness 思维）。
    pub capability_network: bool,
    pub capability_shell: bool,
    pub capability_write_file: bool,
    pub max_tool_iterations: usize,
}

impl Default for PromptContext {
    fn default() -> Self {
        Self {
            run_id: None,
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            capability_network: false,
            capability_shell: false,
            capability_write_file: false,
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
    /// - `HIPPOCAMPUS_CAPABILITY_NETWORK` / `HIPPOCAMPUS_CAPABILITY_SHELL` / `HIPPOCAMPUS_CAPABILITY_WRITE_FILE`：`1/true/yes/on`
    pub fn from_env() -> anyhow::Result<Self> {
        let run_id = std::env::var("HIPPOCAMPUS_RUN_ID").ok().filter(|s| !s.trim().is_empty());

        let workspace_root = match std::env::var("HIPPOCAMPUS_WORKSPACE_ROOT") {
            Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => std::env::current_dir()?,
        };

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
            capability_network: parse_bool_flag(std::env::var("HIPPOCAMPUS_CAPABILITY_NETWORK")),
            capability_shell: parse_bool_flag(std::env::var("HIPPOCAMPUS_CAPABILITY_SHELL")),
            capability_write_file: parse_bool_flag(std::env::var(
                "HIPPOCAMPUS_CAPABILITY_WRITE_FILE",
            )),
            max_tool_iterations: 0,
        })
    }
}

/// 一次 run 的配置。
#[derive(Debug, Clone)]
pub struct ReactConfig {
    /// 最多推理-行动轮数（防止死循环）。
    pub max_steps: usize,
}

impl Default for ReactConfig {
    fn default() -> Self {
        Self { max_steps: 8 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReactError {
    /// 既没有 Final Answer，也解析不出 Action。
    NoActionOrFinal,
    /// 超过 `max_steps`。
    MaxSteps,
}

impl fmt::Display for ReactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoActionOrFinal => write!(
                f,
                "model output contained neither 'Final Answer:' nor a valid Action/Action Input pair"
            ),
            Self::MaxSteps => write!(f, "exceeded max ReAct steps"),
        }
    }
}

impl std::error::Error for ReactError {}

/// 大模型提供方：只负责把 prompt 变成文本输出。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String>;
}

/// 一个最小的“多 provider 路由器”：按名字选择 provider。
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn LlmProvider>>,
    default: String,
}

impl ProviderRegistry {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            providers: HashMap::new(),
            default: default.into(),
        }
    }

    pub fn register(mut self, name: impl Into<String>, provider: Box<dyn LlmProvider>) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    pub fn get(&self, name: Option<&str>) -> Option<&dyn LlmProvider> {
        let key = name.unwrap_or(self.default.as_str());
        self.providers.get(key).map(|p| p.as_ref())
    }
}

/// 工具：名字 + 执行。
pub trait Tool {
    fn name(&self) -> &'static str;
    fn run(&self, input: &str) -> String;
}

/// ReAct 编排器（无状态；工具列表在构造时传入）。
pub struct ReactLoop {
    pub system: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub config: ReactConfig,
}

impl ReactLoop {
    pub fn new(system: impl Into<String>, tools: Vec<Box<dyn Tool>>, config: ReactConfig) -> Self {
        Self {
            system: system.into(),
            tools,
            config,
        }
    }

    /// 渲染一次模型调用将要使用的完整 prompt（用于 `--print-system-prompt` / 单测）。
    pub fn render_prompt(&self, prompt_ctx: &PromptContext, task: &str, scratchpad: &str) -> String {
        let mut ctx = prompt_ctx.clone();
        ctx.max_tool_iterations = self.config.max_steps;
        build_prompt(&self.system, task, scratchpad, &self.tools, &ctx)
    }

    /// 运行 ReAct：把 `task` 和用户可见的 scratchpad 交给模型，直到 Final Answer 或报错。
    pub async fn run(
        &self,
        llm: &dyn LlmProvider,
        prompt_ctx: &PromptContext,
        task: &str,
    ) -> Result<String, ReactError> {
        let mut scratchpad = String::new();

        for _ in 0..self.config.max_steps {
            let mut ctx = prompt_ctx.clone();
            ctx.max_tool_iterations = self.config.max_steps;
            let prompt = build_prompt(&self.system, task, &scratchpad, &self.tools, &ctx);
            let raw = llm
                .complete(&prompt)
                .await
                .unwrap_or_else(|e| format!("Thought: provider_error\nFinal Answer: {e}\n"));
            scratchpad.push_str(raw.trim());
            scratchpad.push('\n');

            if let Some(answer) = parse_final_answer(&raw) {
                return Ok(answer.trim().to_string());
            }

            let Some((tool_name, tool_input)) = parse_action(&raw) else {
                return Err(ReactError::NoActionOrFinal);
            };

            let observation = self.invoke_tool(tool_name, tool_input);
            scratchpad.push_str(&format!("Observation: {observation}\n"));
        }

        Err(ReactError::MaxSteps)
    }

    fn invoke_tool(&self, name: &str, input: &str) -> String {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.run(input))
            .unwrap_or_else(|| format!("unknown tool: {name}"))
    }
}

fn build_prompt(
    system: &str,
    task: &str,
    scratchpad: &str,
    tools: &[Box<dyn Tool>],
    prompt_ctx: &PromptContext,
) -> String {
    let mut tool_lines = String::new();
    for t in tools {
        tool_lines.push_str(&format!("- {}\n", t.name()));
    }

    let static_block = format!(
        "{PROMPT_CORE}\n\n{PROMPT_SAFETY}\n\n__HIPPOCAMPUS_SYSTEM_PROMPT_STATIC_BOUNDARY__\n\n{PROMPT_REACT}"
    );

    let harness = render_harness_section(prompt_ctx);
    let workspace_docs = render_workspace_docs_section(&prompt_ctx.workspace_root);

    // 仍然使用占位符替换（避免 `format!` 模板必须是字面量的限制）。
    let mut tmpl = String::new();
    tmpl.push_str("{static_block}\n\n");
    tmpl.push_str("## Runtime System（来自调用方）\n\n");
    tmpl.push_str("{system}\n\n");
    tmpl.push_str("{harness}\n\n");
    if !workspace_docs.trim().is_empty() {
        tmpl.push_str("{workspace_docs}\n\n");
    }
    tmpl.push_str("可用工具（tool_name 列表）：\n{tool_lines}\n\n");
    tmpl.push_str("任务（Task）：{task}\n\n");
    tmpl.push_str("Scratchpad（历史记录，含 Observation）：\n{scratchpad}\n");

    tmpl.replace("{static_block}", static_block.trim())
        .replace("{system}", system)
        .replace("{harness}", harness.trim())
        .replace("{workspace_docs}", workspace_docs.trim())
        .replace("{tool_lines}", tool_lines.trim_end())
        .replace("{task}", task)
        .replace("{scratchpad}", scratchpad)
}

fn render_harness_section(ctx: &PromptContext) -> String {
    let run_id = ctx
        .run_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("none");
    let root = ctx.workspace_root.display().to_string();
    format!(
        "## Harness Context\n\n\
         - run_id: {run_id}\n\
         - workspace_root: `{root}`\n\
         - max_tool_iterations: {}\n\
         - capabilities:\n\
           - network: {}\n\
           - shell: {}\n\
           - write_file: {}\n",
        ctx.max_tool_iterations,
        on_off(ctx.capability_network),
        on_off(ctx.capability_shell),
        on_off(ctx.capability_write_file),
    )
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}

fn render_workspace_docs_section(workspace_root: &Path) -> String {
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

fn parse_final_answer(text: &str) -> Option<&str> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Final Answer:") {
            return Some(rest);
        }
    }
    None
}

fn parse_action(text: &str) -> Option<(&str, &str)> {
    let mut action: Option<&str> = None;
    let mut action_input: Option<&str> = None;

    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Action:") {
            action = Some(rest.trim());
        } else if let Some(rest) = line.strip_prefix("Action Input:") {
            action_input = Some(rest.trim());
        }
    }

    match (action, action_input) {
        (Some(a), Some(i)) if !a.is_empty() => Some((a, i)),
        _ => None,
    }
}

/// 示例工具：原样返回输入。
pub struct EchoTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn run(&self, input: &str) -> String {
        input.to_string()
    }
}

/// OpenAI-compatible Chat Completions provider（OpenAI/OpenRouter/自建兼容网关均可）。
///
/// 走 `POST {base_url}/v1/chat/completions`，仅取第一条 choice 的 message.content。
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct ChatCompletionsReq<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 1],
    temperature: f32,
}

#[derive(Debug, serde::Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, serde::Deserialize)]
struct ChatCompletionsResp {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, serde::Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, serde::Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = ChatCompletionsReq {
            model: &self.model,
            messages: [ChatMessage {
                role: "user",
                content: prompt,
            }],
            temperature: 0.2,
        };

        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<ChatCompletionsResp>()
            .await?;

        let content = resp
            .choices
            .get(0)
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubLlm {
        outputs: Vec<&'static str>,
        i: usize,
    }

    #[async_trait]
    impl LlmProvider for std::sync::Mutex<StubLlm> {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<String> {
            let mut guard = self.lock().unwrap();
            let s = guard.outputs[guard.i];
            guard.i += 1;
            Ok(s.to_string())
        }
    }

    #[tokio::test]
    async fn stops_on_final_answer_without_tools() {
        let loop_ = ReactLoop::new("You are a test agent.", vec![], ReactConfig { max_steps: 4 });
        let llm = std::sync::Mutex::new(StubLlm {
            outputs: vec!["Thought: done\nFinal Answer: 42\n"],
            i: 0,
        });
        let ctx = PromptContext::default();
        assert_eq!(loop_.run(&llm, &ctx, "what?").await.unwrap(), "42");
    }

    #[tokio::test]
    async fn one_tool_then_final() {
        let loop_ = ReactLoop::new(
            "You are a test agent.",
            vec![Box::new(EchoTool)],
            ReactConfig { max_steps: 4 },
        );
        let llm = std::sync::Mutex::new(StubLlm {
            outputs: vec![
                "Thought: need echo\nAction: echo\nAction Input: hello\n",
                "Thought: got it\nFinal Answer: HELLO\n",
            ],
            i: 0,
        });
        let ctx = PromptContext::default();
        assert_eq!(loop_.run(&llm, &ctx, "echo hello").await.unwrap(), "HELLO");
    }

    #[test]
    fn workspace_agents_md_is_injected_when_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("AGENTS.md"),
            "# Title\n\nhello from agents\n",
        )
        .unwrap();

        let ctx = PromptContext {
            run_id: Some("run-1".into()),
            workspace_root: dir.path().to_path_buf(),
            capability_network: false,
            capability_shell: false,
            capability_write_file: false,
            max_tool_iterations: 3,
        };

        let loop_ = ReactLoop::new(
            "SYS",
            vec![Box::new(EchoTool)],
            ReactConfig { max_steps: 3 },
        );
        let rendered = loop_.render_prompt(&ctx, "TASK", "");
        assert!(rendered.contains("hello from agents"));
        assert!(rendered.contains("run-1"));
        assert!(rendered.contains("max_tool_iterations: 3"));
    }
}
