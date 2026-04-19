use async_trait::async_trait;

use crate::llm::LlmProvider;

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
