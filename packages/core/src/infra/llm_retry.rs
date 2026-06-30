use super::llm::LlmClient;
use super::quota_state::{
    is_exhausted as is_quota_exhausted, set_exhausted as set_quota_exhausted,
};
use crate::error::{InfraError, InfraResult};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_MAX_RETRIES: usize = 5;
pub const DEFAULT_RETRY_BASE_DELAY_SECS: u64 = 10;
pub const DEFAULT_REQUEST_TIMEOUT_MILLIS: u64 = 90_000;

#[derive(Clone, Copy, Debug)]
pub struct LlmRetryPolicy {
    pub max_retries: usize,
    pub base_delay_secs: u64,
    pub request_timeout_millis: u64,
}

impl Default for LlmRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            base_delay_secs: DEFAULT_RETRY_BASE_DELAY_SECS,
            request_timeout_millis: DEFAULT_REQUEST_TIMEOUT_MILLIS,
        }
    }
}

pub async fn llm_with_retry(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
) -> InfraResult<String> {
    llm_with_retry_policy(
        client,
        prompt,
        system,
        LlmRetryPolicy::default(),
        |_a, _m, _d, _e| {},
    )
    .await
}

pub async fn llm_with_retry_policy<F>(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
    policy: LlmRetryPolicy,
    mut on_retry: F,
) -> InfraResult<String>
where
    F: FnMut(usize, usize, u64, &InfraError),
{
    let max_retries = policy.max_retries.max(1);
    let request_timeout = Duration::from_millis(policy.request_timeout_millis.max(1));

    for attempt in 0..max_retries {
        if is_quota_exhausted() {
            return Err(InfraError::RateLimited {
                retry_after_secs: None,
            });
        }

        let result = match tokio::time::timeout(
            request_timeout,
            client.complete(prompt, Some(system)),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(InfraError::LlmRequest(format!(
                "LLM 请求超时: {}ms",
                request_timeout.as_millis()
            ))),
        };

        match result {
            Ok(response) => return Ok(response),
            Err(err @ InfraError::RateLimited { retry_after_secs }) => {
                set_quota_exhausted(retry_after_secs);
                return Err(err);
            }
            Err(err) => {
                if !is_retryable_error(&err) || attempt == max_retries - 1 {
                    return Err(err);
                }

                let delay_secs = backoff_delay_secs(policy.base_delay_secs, attempt);
                on_retry(attempt + 1, max_retries, delay_secs, &err);
                if delay_secs > 0 {
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    unreachable!("retry loop always returns on success or failure")
}

fn is_retryable_error(err: &InfraError) -> bool {
    let msg = err.to_string();
    msg.contains("cooldown")
        || msg.contains("service_busy")
        || msg.contains("rate")
        || msg.contains("429")
        || msg.contains("Upstream")
        || msg.contains("timeout")
        || msg.contains("timed out")
        || msg.contains("请求超时")
        || msg.contains("empty response")
        || msg.contains("stream disconnected before completion")
        || msg.contains("stream closed before")
        || msg.contains("INTERNAL_ERROR; received from peer")
        || msg.contains("internal_server_error")
}

fn backoff_delay_secs(base_delay_secs: u64, attempt: usize) -> u64 {
    let backoff_factor = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
    base_delay_secs.saturating_mul(backoff_factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::quota_state::{is_exhausted as is_quota_exhausted, set_quota_file_override};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{LazyLock, Mutex};
    use tempfile::TempDir;
    use tokio::sync::Mutex as AsyncMutex;

    static QUOTA_TEST_LOCK: LazyLock<AsyncMutex<()>> = LazyLock::new(|| AsyncMutex::new(()));

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

    struct SlowThenOkClient {
        calls: AtomicUsize,
    }

    impl SlowThenOkClient {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl LlmClient for SlowThenOkClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            let call = self.calls.fetch_add(1, Ordering::Relaxed);
            if call == 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok("ok".into())
        }
    }

    struct QuotaTestGuard {
        _dir: TempDir,
    }

    impl QuotaTestGuard {
        fn new() -> Self {
            let dir = TempDir::new().expect("tempdir");
            let path = dir.path().join(".refine").join("quota_exhausted_until");
            set_quota_file_override(Some(path));
            Self { _dir: dir }
        }
    }

    impl Drop for QuotaTestGuard {
        fn drop(&mut self) {
            set_quota_file_override(None::<PathBuf>);
        }
    }

    #[tokio::test]
    async fn retries_on_retryable_error_then_succeeds() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

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
                ..LlmRetryPolicy::default()
            },
            |_attempt, _max_retries, _delay_secs, _err| {},
        )
        .await
        .expect("should succeed after retry");

        assert_eq!(result, "ok");
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

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
                ..LlmRetryPolicy::default()
            },
            |_attempt, _max_retries, _delay_secs, _err| {},
        )
        .await
        .expect_err("non-retryable errors should fail fast");

        assert!(matches!(err, InfraError::LlmRequest(_)));
        assert!(err.to_string().contains("invalid api key"));
        assert_eq!(client.calls(), 1);
    }

    #[tokio::test]
    async fn retries_on_stream_disconnect_then_succeeds() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

        let client = Arc::new(SequenceClient::new(vec![
            Err(InfraError::LlmRequest(
                "stream error: stream disconnected before completion".into(),
            )),
            Ok("ok".into()),
        ]));

        let result = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 3,
                base_delay_secs: 0,
                ..LlmRetryPolicy::default()
            },
            |_attempt, _max, _delay, _err| {},
        )
        .await;

        match result {
            Ok(value) => assert_eq!(value, "ok"),
            Err(err) => panic!("expected retry to succeed, got {err}"),
        }
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn retries_on_stream_internal_error_then_succeeds() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

        let client = Arc::new(SequenceClient::new(vec![
            Err(InfraError::LlmRequest(
                r#"API 错误: {"error":{"message":"stream error: stream ID 3953; INTERNAL_ERROR; received from peer","type":"server_error","param":"","code":"internal_server_error"}}"#.into(),
            )),
            Ok("ok".into()),
        ]));

        let result = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 3,
                base_delay_secs: 0,
                ..LlmRetryPolicy::default()
            },
            |_attempt, _max, _delay, _err| {},
        )
        .await;

        match result {
            Ok(value) => assert_eq!(value, "ok"),
            Err(err) => panic!("expected internal stream error retry to succeed, got {err}"),
        }
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn retries_on_request_timeout_then_succeeds() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

        let client = Arc::new(SlowThenOkClient::new());

        let result = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 3,
                base_delay_secs: 0,
                request_timeout_millis: 10,
            },
            |_attempt, _max, _delay, _err| {},
        )
        .await;

        match result {
            Ok(value) => assert_eq!(value, "ok"),
            Err(err) => panic!("expected timeout retry to succeed, got {err}"),
        }
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn explicit_rate_limit_stops_and_records_quota_state() {
        let _env_guard = QUOTA_TEST_LOCK.lock().await;
        let _quota_guard = QuotaTestGuard::new();

        let client = Arc::new(SequenceClient::new(vec![Err(InfraError::RateLimited {
            retry_after_secs: Some(42),
        })]));

        let err = llm_with_retry_policy(
            &(client.clone() as Arc<dyn LlmClient>),
            "prompt",
            "system",
            LlmRetryPolicy {
                max_retries: 5,
                base_delay_secs: 0,
                ..LlmRetryPolicy::default()
            },
            |_attempt, _max_retries, _delay_secs, _err| {},
        )
        .await
        .expect_err("rate-limited calls should stop immediately");

        assert!(matches!(
            err,
            InfraError::RateLimited {
                retry_after_secs: Some(42)
            }
        ));
        assert_eq!(client.calls(), 1);
        assert!(is_quota_exhausted());
    }
}
