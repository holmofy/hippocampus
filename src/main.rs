//! 演示：
//! - 默认用 OpenAI-compatible provider（OpenAI / OpenRouter / 自建兼容网关）
//! - 若未配置环境变量，则 fallback 到「脚本 LLM」跑通循环（演示 `list_dir`，非 echo）
//! - 启动时从**当前工作目录**读取 `.env` 并注入进程环境（文件不存在则忽略；**不覆盖**已在 shell 里设置的同名变量）
//! - **无位置参数**时进入交互会话：每行输入一个任务目标；**直接回车**退出
//! - 推理链（Thought / Action / Observation）通过 `ReactConfig::trace_thinking` 打到 **stderr**，与最终答案（stdout）分离
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
//! - 位置参数 `TASK...`：非交互，单轮执行后退出

use hippocampus::{
    assemble_tools, LlmProvider, OpenAiCompatibleProvider, PromptContext, ReactConfig, ReactLoop,
    ReactError, ToolProfile,
};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// 面向工作区与工具的通用系统提示（具体格式约束见静态 prompt 模板）。
const AGENT_SYSTEM: &str = r#"You are Hippocampus, a general-purpose autonomous agent operating inside a bounded workspace harness.
- Clarify ambiguities when needed, but prefer acting with tools over guessing when facts are cheap to obtain.
- Use read_file / list_dir / glob_search / grep_search / lsp (when allowed) to inspect the codebase; use echo only for trivial checks.
- Respect capability flags: do not attempt network or shell tools unless the harness enables them.
- Every model turn MUST follow the required scratchpad format: Thought, then either Action+Action Input or Final Answer."#;

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

    /// 交互模式下每轮任务前重置，否则会立刻耗尽脚本。
    async fn rewind(&self) {
        self.0.lock().await.next = 0;
    }
}

#[async_trait::async_trait]
impl LlmProvider for ScriptedProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
        let mut guard = self.0.lock().await;
        let idx = guard.next;
        guard.next += 1;
        let out = guard.lines.get(idx).cloned().unwrap_or_else(|| {
            "Thought: no more scripted turns\nFinal Answer: (scripted LLM exhausted)\n".into()
        });

        eprintln!(
            "[scripted LLM] 第 {} 轮（prompt 前 320 字符预览）\n{}\n",
            idx + 1,
            prompt.chars().take(320).collect::<String>()
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
    /// 非空时表示「单轮 CLI 任务」，否则进入交互会话。
    task: String,
}

fn parse_cli() -> Result<Cli, String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut cli = Cli::default();

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
                    "用法: hippocampus [选项] [TASK...]\n\
                     \n\
                     无 TASK 时进入交互模式：输入任务目标后回车执行，空行退出。\n\
                     有 TASK 时单轮执行后退出。\n\
                     \n\
                     选项:\n\
                       --print-system-prompt\n\
                       --workspace DIR\n\
                       --run-id ID\n\
                       --profile PROFILE\n\
                     \n\
                     环境变量:\n\
                       CODEX_BASE_URL / CODEX_API_KEY / CODEX_MODEL\n\
                       HIPPOCAMPUS_WORKSPACE_ROOT / HIPPOCAMPUS_RUN_ID /\n\
                       HIPPOCAMPUS_TOOL_PROFILE / HIPPOCAMPUS_TOOL_ALLOWLIST /\n\
                       HIPPOCAMPUS_CAPABILITY_* / HIPPOCAMPUS_LSP_COMMAND\n\
                     \n\
                     说明: 思考过程（Thought/Action/Observation）打印到 stderr；最终答案打印到 stdout。"
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

async fn read_user_task() -> std::io::Result<Option<String>> {
    tokio::task::spawn_blocking(|| {
        print!("任务> ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        let n = std::io::stdin().lock().read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let t = line.trim().to_string();
        if t.is_empty() {
            Ok(None)
        } else {
            Ok(Some(t))
        }
    })
    .await
    .unwrap_or_else(|e| std::io::Result::Err(std::io::Error::new(std::io::ErrorKind::Other, e)))
}

fn scripted_demo_lines() -> [&'static str; 2] {
    [
        "Thought: I should list the workspace root to ground my answer in real directory entries.\nAction: list_dir\nAction Input: {\"path\":\".\",\"limit\":32}\n",
        "Thought: The listing observation is enough for a minimal scripted demo.\nFinal Answer: Scripted path completed: workspace root was listed via list_dir; configure CODEX_* for a real model.\n",
    ]
}

enum LlmKind {
    OpenAi(OpenAiCompatibleProvider),
    Scripted(ScriptedProvider),
}

async fn run_react(
    llm: &LlmKind,
    loop_: &ReactLoop,
    ctx: &PromptContext,
    task: &str,
) -> Result<String, ReactError> {
    match llm {
        LlmKind::OpenAi(p) => loop_.run(p, ctx, task).await,
        LlmKind::Scripted(p) => loop_.run(p, ctx, task).await,
    }
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

    let mut prompt_ctx = match PromptContext::from_env() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("无法从环境构造 PromptContext: {e:#}");
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
            eprintln!("工具 profile / 白名单无效: {e:#}");
            std::process::exit(1);
        }
    }

    let tools = match assemble_tools(&prompt_ctx) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("装配工具失败: {e:#}");
            std::process::exit(1);
        }
    };

    let react_cfg = ReactConfig {
        max_steps: 32,
        trace_thinking: true,
    };
    let loop_ = ReactLoop::new(AGENT_SYSTEM, tools, react_cfg);

    let preview_task = if cli.task.trim().is_empty() {
        "（在此输入你的任务；CLI 预览占位）"
    } else {
        cli.task.trim()
    };

    if cli.print_system_prompt {
        println!("{}", loop_.render_prompt(&prompt_ctx, preview_task, ""));
        return;
    }

    let base_url = std::env::var("CODEX_BASE_URL").ok();
    let api_key = std::env::var("CODEX_API_KEY").ok();
    let model = std::env::var("CODEX_MODEL").ok();

    let llm = if let (Some(base_url), Some(api_key), Some(model)) = (base_url, api_key, model) {
        eprintln!("使用 OpenAI-compatible：model={model}\n");
        LlmKind::OpenAi(OpenAiCompatibleProvider::new(base_url, api_key, model))
    } else {
        eprintln!("未设置 CODEX_BASE_URL / CODEX_API_KEY / CODEX_MODEL，使用脚本化 LLM（list_dir 演示）。\n");
        LlmKind::Scripted(ScriptedProvider::new(scripted_demo_lines()))
    };

    let print_run_outcome = |res: Result<String, ReactError>| {
        match res {
            Ok(answer) => {
                println!("\n── 最终答案 ──\n{answer}\n");
            }
            Err(e) => {
                eprintln!("执行失败: {e}");
            }
        }
    };

    if !cli.task.trim().is_empty() {
        print_run_outcome(run_react(&llm, &loop_, &prompt_ctx, cli.task.trim()).await);
        return;
    }

    eprintln!(
        "Hippocampus 交互会话。工作区: {}\n输入任务目标后回车执行；空行退出。\n",
        prompt_ctx.workspace_root.display()
    );

    while let Some(task) = match read_user_task().await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("读取输入失败: {e}");
            None
        }
    } {
        if let LlmKind::Scripted(p) = &llm {
            p.rewind().await;
        }
        let res = run_react(&llm, &loop_, &prompt_ctx, &task).await;
        print_run_outcome(res);
    }

    eprintln!("再见。");
}
