use crate::llm::LlmProvider;
use crate::policy::PromptContext;
use crate::prompting::render;
use crate::tools::{execute_tool_call, Tool};

use super::error::ReactError;
use super::parser::{parse_final_answer, parse_react_tool_call, parse_thought};

/// 一次 run 的配置。
#[derive(Debug, Clone)]
pub struct ReactConfig {
    /// 最多推理-行动轮数（防止死循环）。
    pub max_steps: usize,
    /// 将每步的 Thought / Action / Observation 打到 stderr（便于观察推理链）。
    pub trace_thinking: bool,
}

impl Default for ReactConfig {
    fn default() -> Self {
        Self {
            max_steps: 8,
            trace_thinking: false,
        }
    }
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
        render::build_prompt(&self.system, task, scratchpad, &self.tools, &ctx)
    }

    /// 运行 ReAct：把 `task` 和用户可见的 scratchpad 交给模型，直到 Final Answer 或报错。
    pub async fn run(
        &self,
        llm: &dyn LlmProvider,
        prompt_ctx: &PromptContext,
        task: &str,
    ) -> Result<String, ReactError> {
        let mut scratchpad = String::new();

        for step in 0..self.config.max_steps {
            let mut ctx = prompt_ctx.clone();
            ctx.max_tool_iterations = self.config.max_steps;
            let prompt = render::build_prompt(&self.system, task, &scratchpad, &self.tools, &ctx);
            let raw = llm
                .complete(&prompt)
                .await
                .unwrap_or_else(|e| format!("Thought: provider_error\nFinal Answer: {e}\n"));

            if self.config.trace_thinking {
                eprintln!("\n── Step {} ──", step + 1);
                match parse_thought(&raw) {
                    Some(t) => eprintln!("思考：\n{t}\n"),
                    None => {
                        let t = raw.trim();
                        if t.is_empty() {
                            eprintln!("思考：(空响应)\n");
                        } else {
                            eprintln!("思考：(未解析到 Thought:，原始输出如下)\n{t}\n");
                        }
                    }
                }
            }

            scratchpad.push_str(raw.trim());
            scratchpad.push('\n');

            if let Some(answer) = parse_final_answer(&raw) {
                if self.config.trace_thinking {
                    eprintln!("最终答案：\n{}\n", answer.trim());
                }
                return Ok(answer.trim().to_string());
            }

            let Some(call) = parse_react_tool_call(&raw) else {
                if self.config.trace_thinking {
                    eprintln!("错误：输出中既没有 Final Answer，也无法解析 Action。");
                }
                return Err(ReactError::NoActionOrFinal);
            };

            if self.config.trace_thinking {
                eprintln!("行动：{}\n输入：{}\n", call.name, call.raw_input);
            }

            let record = execute_tool_call(&self.tools, prompt_ctx, call);
            let obs = record.observation_text();
            if self.config.trace_thinking {
                eprintln!("观察：\n{obs}\n");
            }
            scratchpad.push_str(&format!("Observation: {}\n", obs));
        }

        Err(ReactError::MaxSteps)
    }
}
