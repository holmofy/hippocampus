//! 演示：
//! - 默认用 OpenAI-compatible provider（OpenAI / OpenRouter / 自建兼容网关）
//! - 若未配置环境变量，则 fallback 到「脚本 LLM」跑通循环
//! - 启动时从**当前工作目录**读取 `.env` 并注入进程环境（文件不存在则忽略；**不覆盖**已在 shell 里设置的同名变量）
//!
//! 环境变量（OpenAI-compatible）：
//! - CODEX_BASE_URL: 例如 "https://api.openai.com"
//! - CODEX_API_KEY:  你的 key
//! - CODEX_MODEL:    例如 "gpt-4.1-mini"（或任何兼容服务的 model id）
//!
//! 额外环境变量（harness / workspace 注入）：
//! - HIPPOCAMPUS_RUN_ID
//! - HIPPOCAMPUS_WORKSPACE_ROOT
//! - HIPPOCAMPUS_TOOL_PROFILE：`coding` / `office` / `full` / `minimal` / `custom`（默认 `coding`）
//! - HIPPOCAMPUS_TOOL_ALLOWLIST：`custom` 时必填，逗号分隔（如 `echo,net_echo`）
//! - HIPPOCAMPUS_CAPABILITY_NETWORK / HIPPOCAMPUS_CAPABILITY_SHELL / HIPPOCAMPUS_CAPABILITY_WRITE_FILE / HIPPOCAMPUS_CAPABILITY_LSP：`1/true/yes/on`
//! - HIPPOCAMPUS_LSP_COMMAND：LSP 可执行文件（默认 `rust-analyzer`）
//!
//! CLI：
//! - `--print-system-prompt`：打印第一轮完整 prompt（不调用模型）
//! - `--workspace <path>` / `--run-id <id>`：覆盖对应字段
//! - `--profile <coding|office|full|minimal|custom>`：覆盖 `HIPPOCAMPUS_TOOL_PROFILE`（`custom` 仍需环境变量 `HIPPOCAMPUS_TOOL_ALLOWLIST`）

use hippocampus::{
    assemble_tools, LlmProvider, OpenAiCompatibleProvider, PromptContext, ReactConfig, ReactLoop,
    ToolProfile,
};
use std::path::PathBuf;

/// 按顺序返回预设回复，用于本地跑通循环。
struct ScriptedLlm {
    lines: Vec<String>,
    next: usize,
}

impl ScriptedLlm {
    fn new(lines: impl IntoIterator<Item: Into<String>>) -> Self {
        Self {
            lines: lines.into_iter().map(Into::into).collect(),
            next: 0,
        }
    }
}

struct ScriptedProvider(tokio::sync::Mutex<ScriptedLlm>);

impl ScriptedProvider {
    fn new(lines: impl IntoIterator<Item: Into<String>>) -> Self {
        Self(tokio::sync::Mutex::new(ScriptedLlm::new(lines)))
    }
}

#[async_trait::async_trait]
impl LlmProvider for ScriptedProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let mut guard = self.0.lock().await;
        let idx = guard.next;
        guard.next += 1;
        let out = guard
            .lines
            .get(idx)
            .cloned()
            .unwrap_or_else(|| "Thought: stuck\nFinal Answer: (no more scripted replies)\n".into());

        eprintln!("--- prompt (truncated) ---\n{}...\n--- model (scripted) ---\n{}", 
            prompt.chars().take(400).collect::<String>(),
            out
        );
        Ok(out)
    }
}

#[derive(Debug, Default)]
struct Cli {
    print_system_prompt: bool,
    workspace: Option<PathBuf>,
    run_id: Option<String>,
    profile: Option<ToolProfile>,
    task: String,
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut cli = Cli {
        task: "Use the echo tool on the phrase 'ReAct OK', then summarize.".to_string(),
        ..Default::default()
    };

    while let Some(arg) = args.peek().cloned() {
        match arg.as_str() {
            "--print-system-prompt" => {
                cli.print_system_prompt = true;
                let _ = args.next();
            }
            "--workspace" => {
                let _ = args.next();
                let Some(v) = args.next() else {
                    return Err("--workspace requires a path".into());
                };
                if v.trim().is_empty() {
                    return Err("--workspace path cannot be empty".into());
                }
                cli.workspace = Some(PathBuf::from(v));
            }
            "--run-id" => {
                let _ = args.next();
                let Some(v) = args.next() else {
                    return Err("--run-id requires a value".into());
                };
                if v.trim().is_empty() {
                    return Err("--run-id cannot be empty".into());
                }
                cli.run_id = Some(v);
            }
            "--profile" => {
                let _ = args.next();
                let Some(v) = args.next() else {
                    return Err("--profile requires a value (coding|office|full|minimal|custom)".into());
                };
                cli.profile = Some(
                    ToolProfile::parse(&v).map_err(|e| format!("--profile: {e}"))?,
                );
            }
            "-h" | "--help" => {
                return Err(
                    "usage: hippocampus [--print-system-prompt] [--workspace DIR] [--run-id ID] [--profile PROFILE] [TASK]\n\
                     \n\
                     environment:\n\
                       CODEX_BASE_URL / CODEX_API_KEY / CODEX_MODEL\n\
                       HIPPOCAMPUS_WORKSPACE_ROOT / HIPPOCAMPUS_RUN_ID /\n\
                       HIPPOCAMPUS_TOOL_PROFILE / HIPPOCAMPUS_TOOL_ALLOWLIST /\n\
                       HIPPOCAMPUS_CAPABILITY_* / HIPPOCAMPUS_LSP_COMMAND"
                        .into(),
                );
            }
            _ => break,
        }
    }

    let rest: Vec<String> = args.collect();
    if !rest.is_empty() {
        cli.task = rest.join(" ");
    }

    Ok(cli)
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    let cli = match parse_cli() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    let system = "You are a helpful assistant. Follow the output format in the prompt exactly.";

    let mut prompt_ctx = match PromptContext::from_env() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to build PromptContext from env: {e:#}");
            std::process::exit(1);
        }
    };
    if let Some(ws) = cli.workspace.clone() {
        prompt_ctx.workspace_root = ws;
    }
    if let Some(run_id) = cli.run_id.clone() {
        prompt_ctx.run_id = Some(run_id);
    }
    if let Some(profile) = cli.profile {
        if let Err(e) = prompt_ctx.set_tool_profile(profile) {
            eprintln!("Invalid tool profile / allowlist: {e:#}");
            std::process::exit(1);
        }
    }

    let tools = match assemble_tools(&prompt_ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to assemble tools: {e:#}");
            std::process::exit(1);
        }
    };

    let loop_ = ReactLoop::new(
        system,
        tools,
        ReactConfig { max_steps: 6 },
    );

    if cli.print_system_prompt {
        println!("{}", loop_.render_prompt(&prompt_ctx, &cli.task, ""));
        return;
    }

    let base_url = std::env::var("CODEX_BASE_URL").ok();
    let api_key = std::env::var("CODEX_API_KEY").ok();
    let model = std::env::var("CODEX_MODEL").ok();

    if let (Some(base_url), Some(api_key), Some(model)) = (base_url, api_key, model) {
        let llm = OpenAiCompatibleProvider::new(base_url, api_key, model);
        match loop_.run(&llm, &prompt_ctx, &cli.task).await {
            Ok(answer) => println!("\nFinal: {answer}"),
            Err(e) => eprintln!("Error: {e}"),
        }
    } else {
        eprintln!("CODEX_BASE_URL/CODEX_API_KEY/CODEX_MODEL not set; using scripted LLM.\n");
        let llm = ScriptedProvider::new([
            "Thought: User wants echoed text; I will call echo.\nAction: echo\nAction Input: \"ReAct OK\"\n",
            "Thought: Observation confirms success.\nFinal Answer: The echo tool returned the text; ReAct loop works.\n",
        ]);
        match loop_.run(&llm, &prompt_ctx, &cli.task).await {
            Ok(answer) => println!("\nFinal: {answer}"),
            Err(e) => eprintln!("Error: {e}"),
        }
    }
}
