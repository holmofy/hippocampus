use async_trait::async_trait;

/// 大模型提供方：只负责把 prompt 变成文本输出。
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> anyhow::Result<String>;
}
