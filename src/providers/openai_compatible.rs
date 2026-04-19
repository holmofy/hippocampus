use async_trait::async_trait;

use crate::llm::LlmProvider;

/// OpenAI-compatible Chat Completions provider（OpenAI/OpenRouter/自建兼容网关均可）。
///
/// 走 `POST …/chat/completions`。`base_url` 可为 `https://host` 或已带 `/v1` 的 OpenAI-SDK 风格地址（如超算互联网 `https://api.scnet.cn/api/llm/v1`），见 [`chat_completions_url`]。
/// 仅取第一条 choice 的 message.content。
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }
}

/// 兼容两种常见写法：`https://api.example.com` → `…/v1/chat/completions`；
/// `https://api.example.com/.../v1`（OpenAI SDK 的 base_url）→ `…/chat/completions`，避免重复 `/v1/v1`。
fn chat_completions_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") {
        format!("{base}/chat/completions")
    } else {
        format!("{base}/v1/chat/completions")
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
        let url = chat_completions_url(&self.base_url);
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
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);
            anyhow::bail!("HTTP {status} for {url}: {body}");
        }

        let parsed: ChatCompletionsResp = serde_json::from_slice(&bytes)?;

        let content = parsed
            .choices
            .get(0)
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content)
    }
}
