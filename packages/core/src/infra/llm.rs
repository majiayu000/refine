//! LLM 客户端，支持 Claude 和 OpenAI。

use crate::error::{InfraError, InfraResult};
use async_trait::async_trait;
use serde::Deserialize;

/// LLM 客户端接口
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送补全请求
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String>;
}
/// Claude 客户端
pub struct ClaudeClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }
}
#[async_trait]
impl LlmClient for ClaudeClient {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": prompt}]
        });

        if let Some(sys) = system {
            body["system"] = serde_json::json!(sys);
        }

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| InfraError::LlmRequest(e.to_string()))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(InfraError::LlmRequest(format!("API 错误: {}", err)));
        }

        let data: ClaudeResponse = resp
            .json()
            .await
            .map_err(|e| InfraError::LlmParse(e.to_string()))?;

        data.content
            .first()
            .and_then(|c| c.text.clone())
            .ok_or_else(|| InfraError::LlmParse("空响应".into()))
    }
}
#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeContent>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: Option<String>,
}
/// OpenAI 客户端
pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAIClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: "gpt-4o".to_string(),
            base_url: "https://api.openai.com".to_string(),
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }
}
#[async_trait]
impl LlmClient for OpenAIClient {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
        let mut messages = Vec::new();

        if let Some(sys) = system {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
        messages.push(serde_json::json!({"role": "user", "content": prompt}));

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": 4096
        });

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| InfraError::LlmRequest(e.to_string()))?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(InfraError::LlmRequest(format!("API 错误: {}", err)));
        }

        let data: OpenAIResponse = resp
            .json()
            .await
            .map_err(|e| InfraError::LlmParse(e.to_string()))?;

        data.choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| InfraError::LlmParse("空响应".into()))
    }
}
#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
}
/// Mock 客户端（测试用）
#[cfg(test)]
pub struct MockLlmClient {
    response: String,
}

#[cfg(test)]
impl MockLlmClient {
    pub fn new(response: &str) -> Self {
        Self {
            response: response.to_string(),
        }
    }
}
#[cfg(test)]
#[async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
        Ok(self.response.clone())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_client() {
        let client = MockLlmClient::new("test response");
        let result = client.complete("hello", None).await.unwrap();
        assert_eq!(result, "test response");
    }
}
