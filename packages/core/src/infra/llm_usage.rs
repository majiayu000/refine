use crate::error::{InfraError, InfraResult};
use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use uuid::Uuid;

const LEDGER_SCHEMA_VERSION: u8 = 1;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LlmTokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LlmCompletion {
    pub content: String,
    pub usage: Option<LlmTokenUsage>,
}

impl LlmCompletion {
    pub(crate) fn text_only(content: String) -> Self {
        Self {
            content,
            usage: None,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ClaudeResponse {
    content: Vec<ClaudeContent>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize)]
struct ClaudeContent {
    text: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

impl ClaudeResponse {
    pub(crate) fn into_completion(self) -> InfraResult<LlmCompletion> {
        let content = self
            .content
            .first()
            .and_then(|content| content.text.clone())
            .ok_or_else(|| InfraError::LlmParse("空响应".into()))?;
        let usage = self.usage.map(|usage| {
            let total_tokens = usage
                .input_tokens
                .saturating_add(usage.output_tokens)
                .saturating_add(usage.cache_creation_input_tokens.unwrap_or(0))
                .saturating_add(usage.cache_read_input_tokens.unwrap_or(0));
            LlmTokenUsage {
                input_tokens: Some(usage.input_tokens),
                output_tokens: Some(usage.output_tokens),
                total_tokens: Some(total_tokens),
                cache_creation_input_tokens: usage.cache_creation_input_tokens,
                cache_read_input_tokens: usage.cache_read_input_tokens,
                reasoning_tokens: None,
            }
        });
        Ok(LlmCompletion { content, usage })
    }
}

#[derive(Deserialize)]
pub(crate) struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
}

#[derive(Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    prompt_tokens_details: Option<OpenAIPromptTokenDetails>,
    completion_tokens_details: Option<OpenAICompletionTokenDetails>,
}

#[derive(Deserialize)]
struct OpenAIPromptTokenDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OpenAICompletionTokenDetails {
    reasoning_tokens: Option<u64>,
}

impl OpenAIResponse {
    pub(crate) fn into_completion(self) -> InfraResult<LlmCompletion> {
        let content = self
            .choices
            .first()
            .and_then(|choice| choice.message.content.clone())
            .ok_or_else(|| InfraError::LlmParse("空响应".into()))?;
        let usage = self.usage.map(|usage| LlmTokenUsage {
            input_tokens: Some(usage.prompt_tokens),
            output_tokens: Some(usage.completion_tokens),
            total_tokens: Some(usage.total_tokens),
            cache_creation_input_tokens: None,
            cache_read_input_tokens: usage
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens),
            reasoning_tokens: usage
                .completion_tokens_details
                .and_then(|details| details.reasoning_tokens),
        });
        Ok(LlmCompletion { content, usage })
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LlmUsageRecord {
    schema_version: u8,
    recorded_at: String,
    run_id: String,
    call_id: String,
    operation: String,
    attempt: usize,
    client_identity: String,
    prompt_sha256: String,
    system_sha256: String,
    prompt_chars: usize,
    system_chars: usize,
    duration_ms: u64,
    status: String,
    error_kind: Option<String>,
    usage: Option<LlmTokenUsage>,
}

impl LlmUsageRecord {
    pub(crate) fn from_attempt(
        operation: &str,
        attempt: usize,
        client_identity: &str,
        prompt: &str,
        system: &str,
        duration: Duration,
        result: Result<&LlmCompletion, &InfraError>,
    ) -> Self {
        let (status, error_kind, usage) = match result {
            Ok(completion) => ("success".to_string(), None, completion.usage.clone()),
            Err(error) => (
                "error".to_string(),
                Some(error_kind(error).to_string()),
                None,
            ),
        };
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            run_id: run_id().to_string(),
            call_id: Uuid::new_v4().to_string(),
            operation: operation.to_string(),
            attempt,
            client_identity: client_identity.to_string(),
            prompt_sha256: digest(prompt),
            system_sha256: digest(system),
            prompt_chars: prompt.chars().count(),
            system_chars: system.chars().count(),
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            status,
            error_kind,
            usage,
        }
    }
}

pub(crate) fn default_usage_ledger_path() -> InfraResult<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| InfraError::UsageLedger("home directory is unavailable".to_string()))?;
    Ok(home.join(".refine").join("llm-usage.jsonl"))
}

pub(crate) fn append_usage_record(path: &Path, record: &LlmUsageRecord) -> InfraResult<()> {
    let parent = path.parent().ok_or_else(|| {
        InfraError::UsageLedger(format!("ledger path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        InfraError::UsageLedger(format!("create {}: {error}", parent.display()))
    })?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| InfraError::UsageLedger(format!("open {}: {error}", path.display())))?;
    file.lock_exclusive()
        .map_err(|error| InfraError::UsageLedger(format!("lock {}: {error}", path.display())))?;

    let write_result = (|| {
        serde_json::to_writer(&mut file, record)
            .map_err(|error| InfraError::UsageLedger(format!("serialize record: {error}")))?;
        file.write_all(b"\n").map_err(|error| {
            InfraError::UsageLedger(format!("append {}: {error}", path.display()))
        })?;
        file.flush()
            .map_err(|error| InfraError::UsageLedger(format!("flush {}: {error}", path.display())))
    })();
    let unlock_result = FileExt::unlock(&file)
        .map_err(|error| InfraError::UsageLedger(format!("unlock {}: {error}", path.display())));
    write_result.and(unlock_result)
}

fn run_id() -> &'static str {
    static RUN_ID: OnceLock<String> = OnceLock::new();
    RUN_ID.get_or_init(|| Uuid::new_v4().to_string())
}

fn digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn error_kind(error: &InfraError) -> &'static str {
    match error {
        InfraError::Database(_) => "database",
        InfraError::NotFound(_) => "not_found",
        InfraError::Serialization(_) => "serialization",
        InfraError::LlmRequest(_) => "request",
        InfraError::LlmHttp { .. } => "http",
        InfraError::LlmParse(_) => "parse",
        InfraError::UsageLedger(_) => "usage_ledger",
        InfraError::LlmBudgetExceeded { .. } => "budget_exceeded",
        InfraError::LlmRejected { .. } => "rejected",
        InfraError::Http(_) => "transport",
        InfraError::RateLimited { .. } => "rate_limited",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_anthropic_cache_usage_without_losing_counters() {
        let response: ClaudeResponse = serde_json::from_str(
            r#"{"content":[{"text":"ok"}],"usage":{"input_tokens":11,"output_tokens":7,"cache_creation_input_tokens":13,"cache_read_input_tokens":17}}"#,
        )
        .unwrap();
        let usage = response.into_completion().unwrap().usage.unwrap();
        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.cache_creation_input_tokens, Some(13));
        assert_eq!(usage.cache_read_input_tokens, Some(17));
        assert_eq!(usage.total_tokens, Some(48));
    }

    #[test]
    fn parses_openai_cached_and_reasoning_usage() {
        let response: OpenAIResponse = serde_json::from_str(
            r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":101,"completion_tokens":23,"total_tokens":124,"prompt_tokens_details":{"cached_tokens":80},"completion_tokens_details":{"reasoning_tokens":9}}}"#,
        )
        .unwrap();
        let usage = response.into_completion().unwrap().usage.unwrap();
        assert_eq!(usage.input_tokens, Some(101));
        assert_eq!(usage.output_tokens, Some(23));
        assert_eq!(usage.total_tokens, Some(124));
        assert_eq!(usage.cache_read_input_tokens, Some(80));
        assert_eq!(usage.reasoning_tokens, Some(9));
    }

    #[test]
    fn concurrent_appends_are_complete_json_lines_and_redacted() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("llm-usage.jsonl"));
        let handles = (0..12)
            .map(|attempt| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let completion = LlmCompletion {
                        content: "response-secret".to_string(),
                        usage: Some(LlmTokenUsage {
                            input_tokens: Some(10),
                            output_tokens: Some(2),
                            total_tokens: Some(12),
                            ..LlmTokenUsage::default()
                        }),
                    };
                    let record = LlmUsageRecord::from_attempt(
                        "test.operation",
                        attempt + 1,
                        "openai:model:endpoint-sha256:safe",
                        "prompt-secret",
                        "system-secret",
                        Duration::from_millis(5),
                        Ok(&completion),
                    );
                    append_usage_record(&path, &record).unwrap();
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        let contents = std::fs::read_to_string(&*path).unwrap();
        assert_eq!(contents.lines().count(), 12);
        assert!(!contents.contains("prompt-secret"));
        assert!(!contents.contains("system-secret"));
        assert!(!contents.contains("response-secret"));
        for line in contents.lines() {
            let record: LlmUsageRecord = serde_json::from_str(line).unwrap();
            assert_eq!(record.status, "success");
            assert_eq!(record.usage.unwrap().total_tokens, Some(12));
        }
    }
}
