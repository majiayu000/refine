use super::llm::LlmClient;
use super::quota_state::{
    is_exhausted as is_quota_exhausted, set_exhausted as set_quota_exhausted,
};
use crate::error::{InfraError, InfraResult};
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
) -> InfraResult<String> {
    llm_with_retry_policy(client, prompt, system, LlmRetryPolicy::default()).await
}

pub async fn llm_with_retry_notify<F>(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
    on_retry: F,
) -> InfraResult<String>
where
    F: FnMut(usize, usize, u64, &InfraError),
{
    llm_with_retry_policy_notify(client, prompt, system, LlmRetryPolicy::default(), on_retry).await
}

pub async fn llm_with_retry_policy(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
    policy: LlmRetryPolicy,
) -> InfraResult<String> {
    llm_with_retry_policy_notify(client, prompt, system, policy, |_, _, _, _| {}).await
}

pub async fn llm_with_retry_policy_notify<F>(
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

    if is_quota_exhausted() {
        return Err(InfraError::RateLimited {
            retry_after_secs: None,
        });
    }

    for attempt in 0..max_retries {
        match client.complete(prompt, Some(system)).await {
            Ok(response) => return Ok(response),
            Err(InfraError::RateLimited { retry_after_secs }) => {
                set_quota_exhausted(retry_after_secs);
                return Err(InfraError::RateLimited { retry_after_secs });
            }
            Err(err) => {
                if !is_retryable_error(&err.to_string()) || attempt == max_retries - 1 {
                    return Err(err);
                }

                let backoff_factor = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
                let delay_secs = policy.base_delay_secs.saturating_mul(backoff_factor);
                on_retry(attempt + 1, max_retries, delay_secs, &err);

                if delay_secs > 0 {
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    unreachable!("retry loop always returns or exhausts");
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
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;
    use tokio::sync::Mutex as AsyncMutex;

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

    struct EnvGuard {
        home: Option<std::ffi::OsString>,
        quota_backoff: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_temp_home(path: &std::path::Path) -> Self {
            let guard = Self {
                home: std::env::var_os("HOME"),
                quota_backoff: std::env::var_os("REFINE_QUOTA_BACKOFF_SECS"),
            };
            std::env::set_var("HOME", path);
            std::env::remove_var("REFINE_QUOTA_BACKOFF_SECS");
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }

            match &self.quota_backoff {
                Some(value) => std::env::set_var("REFINE_QUOTA_BACKOFF_SECS", value),
                None => std::env::remove_var("REFINE_QUOTA_BACKOFF_SECS"),
            }
        }
    }

    fn quota_test_lock() -> &'static AsyncMutex<()> {
        static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| AsyncMutex::new(()))
    }

    fn quota_file_path(home: &TempDir) -> PathBuf {
        home.path().join(".refine").join("quota_exhausted_until")
    }

    #[tokio::test]
    async fn retries_on_retryable_error_then_succeeds() {
        let _lock = quota_test_lock().lock().await;
        let home = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_temp_home(home.path());

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
        assert!(!quota_file_path(&home).exists());
    }

    #[tokio::test]
    async fn does_not_retry_non_retryable_error() {
        let _lock = quota_test_lock().lock().await;
        let home = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_temp_home(home.path());

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

        assert!(matches!(err, InfraError::LlmRequest(_)));
        assert!(err.to_string().contains("invalid api key"));
        assert_eq!(client.calls(), 1);
        assert!(!quota_file_path(&home).exists());
    }

    #[tokio::test]
    async fn explicit_rate_limited_stops_and_records_quota_state() {
        let _lock = quota_test_lock().lock().await;
        let home = TempDir::new().expect("tempdir");
        let _env = EnvGuard::set_temp_home(home.path());

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
            },
        )
        .await
        .expect_err("rate limit should stop immediately");

        assert!(matches!(
            err,
            InfraError::RateLimited {
                retry_after_secs: Some(42)
            }
        ));
        assert_eq!(client.calls(), 1);
        assert!(quota_file_path(&home).exists());
        assert!(is_quota_exhausted());
    }
}
