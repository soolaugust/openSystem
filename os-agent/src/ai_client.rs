use crate::config::ModelConfig;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// ── OpenAI-compat format ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessageContent,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessageContent {
    content: String,
}

// ── Anthropic native format ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    messages: Vec<Message>,
    max_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

// ── Shared message type ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    #[allow(dead_code)]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AiClient {
    client: Client,
    config: ModelConfig,
    /// true = use Anthropic /v1/messages format; false = use OpenAI /v1/chat/completions
    use_anthropic_format: bool,
}

impl AiClient {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.network.timeout_ms))
            .build()?;
        // Auto-detect: if base_url contains "anthropic" or api_format is "anthropic"
        let use_anthropic_format = config.api.base_url.contains("anthropic")
            || config.api.api_format.as_deref() == Some("anthropic");
        Ok(Self {
            client,
            config,
            use_anthropic_format,
        })
    }

    pub async fn complete(&self, messages: Vec<Message>) -> Result<String> {
        let max_attempts = self.config.network.retry_count.max(1) as usize;
        let mut last_error = String::new();

        for attempt in 1..=max_attempts {
            match self.try_complete(&messages).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    last_error = e.to_string();
                    if attempt < max_attempts {
                        let wait = std::time::Duration::from_millis(500 * attempt as u64);
                        tokio::time::sleep(wait).await;
                    }
                }
            }
        }

        anyhow::bail!(
            "LLM API failed after {} attempts: {}",
            max_attempts,
            last_error
        )
    }

    async fn try_complete(&self, messages: &[Message]) -> Result<String> {
        if self.use_anthropic_format {
            self.try_complete_anthropic(messages).await
        } else {
            self.try_complete_openai(messages).await
        }
    }

    async fn try_complete_openai(&self, messages: &[Message]) -> Result<String> {
        let req = OpenAiRequest {
            model: self.config.api.model.clone(),
            messages: messages.to_vec(),
            temperature: 0.2,
            max_tokens: Some(4096),
        };

        let response = self
            .client
            .post(format!("{}/chat/completions", self.config.api.base_url))
            .header(
                "Authorization",
                format!("Bearer {}", self.config.api.api_key),
            )
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM API returned {}: {}", status, body);
        }

        let chat_response: OpenAiResponse = response
            .json()
            .await
            .context("Failed to parse LLM API response")?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .context("Empty response from LLM API")
    }

    async fn try_complete_anthropic(&self, messages: &[Message]) -> Result<String> {
        // Anthropic API: system messages are a top-level field, not in the messages array
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());
        let non_system: Vec<Message> = messages
            .iter()
            .filter(|m| m.role != "system")
            .cloned()
            .collect();

        let req = AnthropicRequest {
            model: self.config.api.model.clone(),
            system,
            messages: non_system,
            max_tokens: 4096,
        };

        let response = self
            .client
            .post(format!("{}/messages", self.config.api.base_url))
            .header("x-api-key", &self.config.api.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API returned {}: {}", status, body);
        }

        let resp: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic API response")?;

        resp.content
            .into_iter()
            .find(|c| c.kind == "text")
            .and_then(|c| c.text)
            .context("Empty response from Anthropic API")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_system() {
        let msg = Message::system("You are a helpful assistant");
        assert_eq!(msg.role, "system");
        assert_eq!(msg.content, "You are a helpful assistant");
    }

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there");
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there");
    }

    #[test]
    fn test_message_serde_roundtrip() {
        let msg = Message::user("test content");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, "user");
        assert_eq!(parsed.content, "test content");
    }

    #[test]
    fn test_message_from_json_literal() {
        let json = r#"{"role":"system","content":"be helpful"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "system");
        assert_eq!(msg.content, "be helpful");
    }

    #[test]
    fn test_message_clone() {
        let msg = Message::user("original");
        let cloned = msg.clone();
        assert_eq!(cloned.role, msg.role);
        assert_eq!(cloned.content, msg.content);
    }
}
