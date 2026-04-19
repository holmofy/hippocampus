use crate::policy::PromptContext;
use crate::tools::Tool;

use super::{PROMPT_CORE, PROMPT_REACT, PROMPT_SAFETY};
use super::workspace;

pub fn build_prompt(
    system: &str,
    task: &str,
    scratchpad: &str,
    tools: &[Box<dyn Tool>],
    prompt_ctx: &PromptContext,
) -> String {
    let mut tool_lines = String::new();
    for t in tools {
        tool_lines.push_str(&format!("- {}: {}\n", t.name(), t.description()));
        tool_lines.push_str("  input_json_schema:\n");
        tool_lines.push_str(&indent_json(
            &serde_json::to_string_pretty(t.input_schema())
                .unwrap_or_else(|_| "{}".into()),
            "  ",
        ));
        tool_lines.push('\n');
    }

    let static_block = format!(
        "{PROMPT_CORE}\n\n{PROMPT_SAFETY}\n\n__HIPPOCAMPUS_SYSTEM_PROMPT_STATIC_BOUNDARY__\n\n{PROMPT_REACT}"
    );

    let harness = render_harness_section(prompt_ctx);
    let workspace_docs = workspace::render_workspace_docs_section(&prompt_ctx.workspace_root);

    // 仍然使用占位符替换（避免 `format!` 模板必须是字面量的限制）。
    let mut tmpl = String::new();
    tmpl.push_str("{static_block}\n\n");
    tmpl.push_str("## Runtime System（来自调用方）\n\n");
    tmpl.push_str("{system}\n\n");
    tmpl.push_str("{harness}\n\n");
    if !workspace_docs.trim().is_empty() {
        tmpl.push_str("{workspace_docs}\n\n");
    }
    tmpl.push_str("可用工具（tool_name + schema）：\n{tool_lines}\n\n");
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

fn indent_json(json: &str, indent: &str) -> String {
    json.lines()
        .map(|line| format!("{indent}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_harness_section(ctx: &PromptContext) -> String {
    let run_id = ctx
        .run_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("none");
    let root = ctx.workspace_root.display().to_string();
    let tools_csv = ctx.allowed_tool_names.join(", ");
    format!(
        "## Harness Context\n\n\
         - run_id: {run_id}\n\
         - workspace_root: `{root}`\n\
         - tool_profile: {}\n\
         - allowed_tools: {tools_csv}\n\
         - max_tool_iterations: {}\n\
         - capabilities:\n\
           - network: {}\n\
           - shell: {}\n\
           - write_file: {}\n\
           - lsp: {}\n",
        ctx.tool_profile,
        ctx.max_tool_iterations,
        on_off(ctx.capability_network),
        on_off(ctx.capability_shell),
        on_off(ctx.capability_write_file),
        on_off(ctx.capability_lsp),
    )
}

fn on_off(v: bool) -> &'static str {
    if v {
        "on"
    } else {
        "off"
    }
}
