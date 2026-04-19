use async_trait::async_trait;

use crate::llm::LlmProvider;
use crate::policy::{PromptContext, ToolProfile};
use crate::react::parser::parse_thought;
use crate::react::{ReactConfig, ReactLoop};
use crate::tools::{assemble_tools, EchoTool, NetworkEchoTool};

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
    let loop_ = ReactLoop::new(
        "You are a test agent.",
        vec![],
        ReactConfig {
            max_steps: 4,
            ..Default::default()
        },
    );
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
        ReactConfig {
            max_steps: 4,
            ..Default::default()
        },
    );
    let llm = std::sync::Mutex::new(StubLlm {
        outputs: vec![
            "Thought: need echo\nAction: echo\nAction Input: \"hello\"\n",
            "Thought: got it\nFinal Answer: hello\n",
        ],
        i: 0,
    });
    let ctx = PromptContext::default();
    assert_eq!(loop_.run(&llm, &ctx, "echo hello").await.unwrap(), "hello");
}

#[tokio::test]
async fn network_tool_is_denied_without_capability() {
    let loop_ = ReactLoop::new(
        "You are a test agent.",
        vec![Box::new(NetworkEchoTool)],
        ReactConfig {
            max_steps: 4,
            ..Default::default()
        },
    );
    let llm = std::sync::Mutex::new(StubLlm {
        outputs: vec![
            "Thought: need net echo\nAction: net_echo\nAction Input: {\"url\":\"https://example.com\"}\n",
            "Thought: policy denied as expected\nFinal Answer: denied\n",
        ],
        i: 0,
    });
    let ctx = PromptContext::default();
    assert_eq!(loop_.run(&llm, &ctx, "call net").await.unwrap(), "denied");
}

#[tokio::test]
async fn lsp_tool_is_denied_without_capability() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "fn main() {}\n").unwrap();
    let mut ctx = PromptContext::default();
    ctx.workspace_root = dir.path().to_path_buf();
    ctx.allowed_tool_names = vec!["lsp".into()];
    ctx.capability_lsp = false;
    let tools = assemble_tools(&ctx).unwrap();
    let loop_ = ReactLoop::new(
        "You are a test agent.",
        tools,
        ReactConfig {
            max_steps: 4,
            ..Default::default()
        },
    );
    let llm = std::sync::Mutex::new(StubLlm {
        outputs: vec![
            "Thought: jump to def\nAction: lsp\nAction Input: {\"operation\":\"go_to_definition\",\"path\":\"lib.rs\",\"line\":1,\"character\":4}\n",
            "Thought: denied as expected\nFinal Answer: denied\n",
        ],
        i: 0,
    });
    assert_eq!(
        loop_.run(&llm, &ctx, "lsp").await.unwrap(),
        "denied"
    );
}

#[test]
fn workspace_agents_md_is_injected_when_present() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("AGENTS.md"),
        "# Title\n\nhello from agents\n",
    )
    .unwrap();

    let mut ctx = PromptContext::default();
    ctx.run_id = Some("run-1".into());
    ctx.workspace_root = dir.path().to_path_buf();
    ctx.max_tool_iterations = 3;

    let loop_ = ReactLoop::new(
        "SYS",
        assemble_tools(&ctx).unwrap(),
        ReactConfig {
            max_steps: 3,
            ..Default::default()
        },
    );
    let rendered = loop_.render_prompt(&ctx, "TASK", "");
    assert!(rendered.contains("hello from agents"));
    assert!(rendered.contains("run-1"));
    assert!(rendered.contains("max_tool_iterations: 3"));
}

#[test]
fn harness_shows_tool_profile_and_allowlist() {
    let mut ctx = PromptContext::default();
    ctx.set_tool_profile(ToolProfile::Office).unwrap();
    let tools = assemble_tools(&ctx).unwrap();
    let loop_ = ReactLoop::new(
        "SYS",
        tools,
        ReactConfig {
            max_steps: 3,
            ..Default::default()
        },
    );
    let rendered = loop_.render_prompt(&ctx, "TASK", "");
    assert!(rendered.contains("tool_profile: office"));
    assert!(rendered.contains(
        "allowed_tools: echo, glob_search, grep_search, list_dir, lsp, net_echo, read_file"
    ));
}

#[test]
fn read_file_respects_line_window() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "L1\nL2\nL3\n").unwrap();
    let mut ctx = PromptContext::default();
    ctx.workspace_root = dir.path().to_path_buf();
    ctx.allowed_tool_names = vec!["read_file".into()];
    let tools = assemble_tools(&ctx).unwrap();
    let rf = tools
        .iter()
        .find(|t| t.name() == "read_file")
        .expect("read_file");
    let out = rf
        .run_json(serde_json::json!({"path": "a.txt", "offset": 2, "limit": 2}))
        .unwrap();
    assert_eq!(out["total_lines"], 3);
    let lines = out["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].as_str().unwrap(), "L2");
    assert_eq!(lines[1].as_str().unwrap(), "L3");
}

#[test]
fn parse_thought_stops_before_action_or_final() {
    let block = "Thought: a\nb\nAction: echo\nAction Input: \"x\"\n";
    assert_eq!(parse_thought(block).as_deref(), Some("a\nb"));
    let fin = "Thought: done\nFinal Answer: 99\n";
    assert_eq!(parse_thought(fin).as_deref(), Some("done"));
}

#[test]
fn read_file_rejects_escape_from_workspace() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    let mut ctx = PromptContext::default();
    ctx.workspace_root = dir.path().to_path_buf();
    ctx.allowed_tool_names = vec!["read_file".into()];
    let tools = assemble_tools(&ctx).unwrap();
    let rf = tools.iter().find(|t| t.name() == "read_file").unwrap();
    let err = rf
        .run_json(serde_json::json!({"path": "../a.txt"}))
        .unwrap_err();
    let s = format!("{err:#}");
    assert!(
        s.contains("workspace") || s.contains("path"),
        "unexpected error: {s}"
    );
}
