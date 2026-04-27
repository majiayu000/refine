use anyhow::Result;
use refine_core::error::InfraError;
use refine_core::infra::{is_quota_exhausted, set_quota_exhausted, LlmClient};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_MAX_RETRIES: usize = 5;
pub const DEFAULT_RETRY_BASE_DELAY_SECS: u64 = 10;

#[derive(Clone, Copy, Debug)]
pub struct LlmRetryPolicy {
    pub max_retries: usize,
    pub base_delay_secs: u64,
}

impl Default for LlmRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay_secs: DEFAULT_RETRY_BASE_DELAY_SECS,
        }
    }
}

pub async fn llm_with_retry(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
) -> Result<String> {
    llm_with_retry_policy(client, prompt, system, LlmRetryPolicy::default()).await
}

pub async fn llm_with_retry_policy(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
    policy: LlmRetryPolicy,
) -> Result<String> {
    let max_retries = policy.max_retries.max(1);
    let mut last_err = String::new();

    for attempt in 0..max_retries {
        if is_quota_exhausted() {
            return Err(anyhow::anyhow!("LLM quota exhausted — skipping call"));
        }

        match client.complete(prompt, Some(system)).await {
            Ok(response) => return Ok(response),
            Err(InfraError::RateLimited { retry_after_secs }) => {
                set_quota_exhausted(retry_after_secs);
                return Err(anyhow::anyhow!(
                    "LLM quota exhausted (retry_after: {:?}s)",
                    retry_after_secs
                ));
            }
            Err(e) => {
                last_err = e.to_string();
                if !is_retryable_error(&last_err) || attempt == max_retries - 1 {
                    break;
                }

                let backoff_factor = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
                let delay = policy.base_delay_secs.saturating_mul(backoff_factor);
                eprintln!(
                    "    Retry ({}/{}) waiting {}s...",
                    attempt + 1,
                    max_retries,
                    delay
                );
                if delay > 0 {
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "LLM call failed ({} retries): {}",
        max_retries,
        last_err
    ))
}

fn is_retryable_error(err: &str) -> bool {
    err.contains("cooldown")
        || err.contains("service_busy")
        || err.contains("rate")
        || err.contains("429")
        || err.contains("Upstream")
        || err.contains("timeout")
        || err.contains("empty response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use refine_core::error::{InfraError, InfraResult};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct SequenceClient {
        calls: AtomicUsize,
        responses: Mutex<VecDeque<InfraResult<String>>>,
    }

    impl SequenceClient {
        fn new(responses: Vec<InfraResult<String>>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                responses: Mutex::new(VecDeque::from(responses)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl LlmClient for SequenceClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.responses
                .lock()
                .expect("lock poisoned")
                .pop_front()
                .unwrap_or_else(|| Err(InfraError::LlmRequest("no queued response".into())))
        }
    }

    #[tokio::test]
    async fn retries_on_retryable_error_then_succeeds() {
        let client = Arc::new(SequenceClient::new(vec![
            Err(InfraError::LlmRequest("429 rate limit".into())),
            Ok("ok".into()),
        ]));

        let result = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 3,
                base_delay_secs: 0,
            },
        )
        .await
        .expect("should succeed after retry");

        assert_eq!(result, "ok");
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let client = Arc::new(SequenceClient::new(vec![
            Err(InfraError::LlmRequest("invalid api key".into())),
            Ok("unused".into()),
        ]));

        let err = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 5,
                base_delay_secs: 0,
            },
        )
        .await
        .expect_err("non-retryable errors should fail fast");

        assert!(err.to_string().contains("invalid api key"));
        assert_eq!(client.calls(), 1);
    }

    #[tokio::test]
    async fn stops_after_max_retries() {
        let client = Arc::new(SequenceClient::new(vec![
            Err(InfraError::LlmRequest("timeout".into())),
            Err(InfraError::LlmRequest("timeout".into())),
            Err(InfraError::LlmRequest("timeout".into())),
            Ok("unused".into()),
        ]));

        let err = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 3,
                base_delay_secs: 0,
            },
        )
        .await
        .expect_err("should fail when retries exhausted");

        assert!(err.to_string().contains("3 retries"));
        assert_eq!(client.calls(), 3);
    }
}
