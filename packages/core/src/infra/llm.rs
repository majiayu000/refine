//! LLM 客户端，支持 Claude 和 OpenAI。

use crate::error::{InfraError, InfraResult};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::SystemTime;

/// LLM 客户端接口
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发送补全请求
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String>;

    /// Stable identity for cache invalidation. Implementations should include
    /// provider, model, and compatible endpoint identity, but never secrets.
    fn cache_identity(&self) -> String {
        "unknown-llm".to_string()
    }
}

/// 从环境变量构建 LLM 客户端。
///
/// 优先级：
/// 1. Anthropic (`REFINE_ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`)
/// 2. OpenAI-compatible (`REFINE_OPENAI_API_KEY` / `OPENAI_API_KEY` / `BASE_API_KEY`)
pub fn build_llm_client_from_env() -> Option<Arc<dyn LlmClient>> {
    if let Some(config) = anthropic_config_from_env() {
        let mut client = ClaudeClient::new(&config.api_key);
        if let Some(model) = config.model {
            client = client.with_model(&model);
        }
        if let Some(base_url) = config.base_url {
            client = client.with_base_url(&base_url);
        }
        return Some(Arc::new(client));
    }

    if let Some(config) = openai_config_from_env() {
        let mut client = OpenAIClient::new(&config.api_key);
        if let Some(model) = config.model {
            client = client.with_model(&model);
        }
        if let Some(base_url) = config.base_url {
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
    fn cache_identity(&self) -> String {
        format!(
            "anthropic:{}:{}",
            self.model,
            endpoint_identity(&self.base_url)
        )
    }

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
            let status = resp.status();
            let retry_after_secs =
                parse_retry_after(resp.headers().get(reqwest::header::RETRY_AFTER));
            let err = resp.text().await.unwrap_or_default();
            return Err(classify_provider_error(status, retry_after_secs, &err));
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
        self.base_url = normalize_openai_base_url(url);
        self
    }
}
#[async_trait]
impl LlmClient for OpenAIClient {
    fn cache_identity(&self) -> String {
        format!(
            "openai:{}:{}",
            self.model,
            endpoint_identity(&self.base_url)
        )
    }

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
            let status = resp.status();
            let retry_after_secs =
                parse_retry_after(resp.headers().get(reqwest::header::RETRY_AFTER));
            let err = resp.text().await.unwrap_or_default();
            return Err(classify_provider_error(status, retry_after_secs, &err));
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

fn classify_provider_error(
    status: reqwest::StatusCode,
    retry_after_secs: Option<u64>,
    body: &str,
) -> InfraError {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return InfraError::RateLimited { retry_after_secs };
    }

    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(|value| value.as_str())
        .unwrap_or(body)
        .trim();

    if status.is_client_error()
        && status != reqwest::StatusCode::REQUEST_TIMEOUT
        && is_content_rejection(code, message)
    {
        return InfraError::LlmRejected {
            code: if code.is_empty() {
                "content_rejected".to_string()
            } else {
                code.to_string()
            },
            message: message.to_string(),
        };
    }

    InfraError::LlmHttp {
        status: status.as_u16(),
        message: body.to_string(),
    }
}

fn parse_retry_after(value: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let value = value?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }

    let retry_at = httpdate::parse_http_date(value).ok()?;
    Some(
        retry_at
            .duration_since(SystemTime::now())
            .unwrap_or_default()
            .as_secs(),
    )
}

fn is_content_rejection(code: &str, message: &str) -> bool {
    let code = code.trim().to_ascii_lowercase();
    matches!(
        code.as_str(),
        "sensitive_words_detected" | "content_filter" | "content_policy_violation"
    ) || message
        .to_ascii_lowercase()
        .contains("sensitive_words_detected")
}

fn endpoint_identity(endpoint: &str) -> String {
    let digest = Sha256::digest(endpoint.as_bytes());
    format!("endpoint-sha256:{digest:x}")
}

struct LlmEnvConfig {
    api_key: String,
    model: Option<String>,
    base_url: Option<String>,
}

fn anthropic_config_from_env() -> Option<LlmEnvConfig> {
    Some(LlmEnvConfig {
        api_key: env_var(&[
            "REFINE_ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ])?,
        model: env_var(&["REFINE_ANTHROPIC_MODEL"]),
        base_url: env_var(&["REFINE_ANTHROPIC_BASE_URL", "ANTHROPIC_BASE_URL"]),
    })
}

fn openai_config_from_env() -> Option<LlmEnvConfig> {
    Some(LlmEnvConfig {
        api_key: env_var(&["REFINE_OPENAI_API_KEY", "OPENAI_API_KEY", "BASE_API_KEY"])?,
        model: env_var(&["REFINE_OPENAI_MODEL", "BASE_MODEL"]),
        base_url: env_var(&["REFINE_OPENAI_BASE_URL", "BASE_URL"]),
    })
}

fn normalize_openai_base_url(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .strip_suffix("/v1")
        .unwrap_or_else(|| url.trim().trim_end_matches('/'))
        .to_string()
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    const LLM_ENV_KEYS: &[&str] = &[
        "REFINE_ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "REFINE_ANTHROPIC_MODEL",
        "REFINE_ANTHROPIC_BASE_URL",
        "ANTHROPIC_BASE_URL",
        "REFINE_OPENAI_API_KEY",
        "OPENAI_API_KEY",
        "REFINE_OPENAI_MODEL",
        "REFINE_OPENAI_BASE_URL",
        "BASE_API_KEY",
        "BASE_MODEL",
        "BASE_URL",
    ];

    #[tokio::test]
    async fn test_mock_client() {
        let client = MockLlmClient::new("test response");
        let result = client.complete("hello", None).await.unwrap();
        assert_eq!(result, "test response");
    }

    #[test]
    fn openai_config_accepts_base_aliases() {
        let Ok(_env_lock) = ENV_LOCK.lock() else {
            panic!("failed to lock env");
        };
        with_clean_llm_env(|| {
            std::env::set_var("BASE_API_KEY", "base-key");
            std::env::set_var("BASE_MODEL", "base-model");
            std::env::set_var("BASE_URL", "https://example.test/openai");

            let Some(config) = openai_config_from_env() else {
                panic!("missing openai config");
            };
            assert_eq!(config.api_key, "base-key");
            assert_eq!(config.model.as_deref(), Some("base-model"));
            assert_eq!(
                config.base_url.as_deref(),
                Some("https://example.test/openai")
            );
        });
    }

    #[test]
    fn refine_openai_vars_override_base_aliases() {
        let Ok(_env_lock) = ENV_LOCK.lock() else {
            panic!("failed to lock env");
        };
        with_clean_llm_env(|| {
            std::env::set_var("REFINE_OPENAI_API_KEY", "refine-key");
            std::env::set_var("REFINE_OPENAI_MODEL", "refine-model");
            std::env::set_var("REFINE_OPENAI_BASE_URL", "https://refine.example.test");
            std::env::set_var("BASE_API_KEY", "base-key");
            std::env::set_var("BASE_MODEL", "base-model");
            std::env::set_var("BASE_URL", "https://base.example.test");

            let Some(config) = openai_config_from_env() else {
                panic!("missing openai config");
            };
            assert_eq!(config.api_key, "refine-key");
            assert_eq!(config.model.as_deref(), Some("refine-model"));
            assert_eq!(
                config.base_url.as_deref(),
                Some("https://refine.example.test")
            );
        });
    }

    #[test]
    fn normalize_openai_base_url_accepts_root_or_v1_endpoint() {
        assert_eq!(
            normalize_openai_base_url("https://api.example.test"),
            "https://api.example.test"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.example.test/v1"),
            "https://api.example.test"
        );
        assert_eq!(
            normalize_openai_base_url("https://api.example.test/openai/v1/"),
            "https://api.example.test/openai"
        );
    }

    #[test]
    fn sensitive_words_error_is_structured_as_non_retryable_rejection() {
        let error = classify_provider_error(
            reqwest::StatusCode::BAD_REQUEST,
            None,
            r#"{"error":{"message":"sensitive_words_detected","code":"sensitive_words_detected"}}"#,
        );
        assert!(matches!(
            error,
            InfraError::LlmRejected { ref code, .. } if code == "sensitive_words_detected"
        ));
    }

    #[test]
    fn sensitive_words_message_without_code_is_still_non_retryable() {
        let error = classify_provider_error(
            reqwest::StatusCode::BAD_REQUEST,
            None,
            r#"{"error":{"message":"request blocked: sensitive_words_detected"}}"#,
        );
        assert!(matches!(
            error,
            InfraError::LlmRejected { ref code, .. } if code == "content_rejected"
        ));
    }

    #[test]
    fn unknown_provider_error_stays_request_error() {
        let error = classify_provider_error(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            None,
            r#"{"error":{"message":"upstream unavailable","code":"upstream_error"}}"#,
        );
        assert!(matches!(error, InfraError::LlmHttp { status: 503, .. }));
    }

    #[test]
    fn provider_rate_limit_is_structured_and_drops_response_body() {
        let error = classify_provider_error(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            Some(42),
            r#"{"error":{"message":"secret provider body","code":"rate_limit"}}"#,
        );
        assert!(matches!(
            error,
            InfraError::RateLimited {
                retry_after_secs: Some(42)
            }
        ));
    }

    #[test]
    fn retry_after_supports_delta_seconds_http_date_and_invalid_values() {
        let seconds = reqwest::header::HeaderValue::from_static("42");
        assert_eq!(parse_retry_after(Some(&seconds)), Some(42));

        let future = SystemTime::now() + std::time::Duration::from_secs(120);
        let http_date =
            reqwest::header::HeaderValue::from_str(&httpdate::fmt_http_date(future)).unwrap();
        let parsed = parse_retry_after(Some(&http_date)).expect("HTTP-date should parse");
        assert!((118..=120).contains(&parsed), "unexpected delay: {parsed}");

        let past = SystemTime::now() - std::time::Duration::from_secs(60);
        let past_date =
            reqwest::header::HeaderValue::from_str(&httpdate::fmt_http_date(past)).unwrap();
        assert_eq!(parse_retry_after(Some(&past_date)), Some(0));

        let invalid = reqwest::header::HeaderValue::from_static("not-a-delay");
        assert_eq!(parse_retry_after(Some(&invalid)), None);
        assert_eq!(parse_retry_after(None), None);
    }

    #[tokio::test]
    async fn concrete_clients_preserve_retry_after_from_provider_429() {
        for provider in ["anthropic", "openai"] {
            let base_url = spawn_single_response_server(
                "429 Too Many Requests",
                &[("Retry-After", "42")],
                r#"{"error":{"message":"do not persist this body"}}"#,
            );
            let client: Box<dyn LlmClient> = match provider {
                "anthropic" => Box::new(ClaudeClient::new("test-key").with_base_url(&base_url)),
                "openai" => Box::new(OpenAIClient::new("test-key").with_base_url(&base_url)),
                _ => unreachable!(),
            };

            let error = client
                .complete("hello", Some("system"))
                .await
                .expect_err("provider 429 must fail");
            assert!(
                matches!(
                    error,
                    InfraError::RateLimited {
                        retry_after_secs: Some(42)
                    }
                ),
                "{provider} returned unexpected error: {error}"
            );
            assert!(!error.to_string().contains("do not persist this body"));
        }
    }

    #[test]
    fn generic_moderation_message_from_5xx_is_not_quarantined() {
        let error = classify_provider_error(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            None,
            r#"{"error":{"message":"moderation service unavailable"}}"#,
        );
        assert!(matches!(error, InfraError::LlmHttp { status: 503, .. }));
    }

    #[test]
    fn openai_content_filter_code_is_quarantined() {
        let error = classify_provider_error(
            reqwest::StatusCode::BAD_REQUEST,
            None,
            r#"{"error":{"message":"blocked","code":"content_filter"}}"#,
        );
        assert!(
            matches!(error, InfraError::LlmRejected { ref code, .. } if code == "content_filter")
        );
    }

    #[test]
    fn cache_identity_never_exposes_endpoint_credentials_or_query() {
        let client = OpenAIClient::new("key")
            .with_base_url("https://user:secret@example.test/openai?token=private-value");
        let identity = client.cache_identity();
        assert!(identity.contains("endpoint-sha256:"));
        for secret in ["user", "secret", "token", "private-value"] {
            assert!(!identity.contains(secret));
        }
    }

    fn spawn_single_response_server(status: &str, headers: &[(&str, &str)], body: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let status = status.to_string();
        let headers = headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        let body = body.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept provider request");
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request).expect("read provider request");
            let mut response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (name, value) in headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(&body);
            stream
                .write_all(response.as_bytes())
                .expect("write provider response");
        });
        format!("http://{address}")
    }

    fn with_clean_llm_env(test: impl FnOnce()) {
        let saved = LLM_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect::<Vec<_>>();

        for key in LLM_ENV_KEYS {
            std::env::remove_var(key);
        }

        test();

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
