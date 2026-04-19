use serde_json::Value;

use crate::policy::ToolCategory;

/// 工具：名字 + JSON Schema（真相来源）+ JSON 入参执行。
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    /// 执行该工具所需的 capabilities（执行期强制；与 `PromptContext` 对齐）。
    fn required_capabilities(&self) -> &'static [ToolCategory];

    /// JSON Schema（Draft-07 兼容子集）：用于执行前参数校验。
    fn input_schema(&self) -> &Value;

    /// 执行工具：入参已通过 schema 校验。
    fn run_json(&self, input: Value) -> anyhow::Result<Value>;
}
