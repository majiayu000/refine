//! LLM 客户端，支持 Claude 和 OpenAI。

use crate::error::{InfraError, InfraResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

/// LLM 客户端接口
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送补全请求
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String>;
}

/// 从环境变量构建 LLM 客户端。
///
/// 优先级：
/// 1. Anthropic (`REFINE_ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`)
/// 2. OpenAI (`REFINE_OPENAI_API_KEY` / `OPENAI_API_KEY`)
pub fn build_llm_client_from_env() -> Option<Arc<dyn LlmClient>> {
    if let Some(api_key) = env_var(&[
        "REFINE_ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
    ]) {
        let mut client = ClaudeClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_ANTHROPIC_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_ANTHROPIC_BASE_URL", "ANTHROPIC_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Some(Arc::new(client));
    }

    if let Some(api_key) = env_var(&["REFINE_OPENAI_API_KEY", "OPENAI_API_KEY"]) {
        let mut client = OpenAIClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_OPENAI_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_OPENAI_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Some(Arc::new(client));
    }

    None
}

pub fn build_required_llm_client_from_env() -> Result<Arc<dyn LlmClient>, String> {
    build_llm_client_from_env().ok_or_else(|| "missing API key".to_string())
}
/// Claude 客户端
pub struct ClaudeClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl ClaudeClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: "claude-opus-4-6".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.trim_end_matches('/').to_string();
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
            .post(format!("{}/v1/messages", self.base_url))
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

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
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
