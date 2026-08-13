//! 提炼用例（应用层编排）
//!
//! 在不引入新 crate 的过渡阶段，将 LLM 调用、JSON 修复集中，
//! 供 server/desktop/cli 复用，避免多处重复实现。

use crate::error::{DomainError, DomainResult, InfraResult};
use crate::infra::{llm_with_retry_policy_ref, LlmClient, LlmRetryBehavior, LlmRetryPolicy};
use crate::knowledge::{Document, DocumentId, DocumentRepository, Item, Source};
use crate::refinement::{
    Conversation, ExtractionPolicy, ExtractionResult, Extractor, PromptTemplate,
};

pub const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";
pub const JSON_REPAIR_SYSTEM_PROMPT: &str =
    "你是 JSON 修复器。只输出一个合法 JSON 对象，不要输出 markdown 或解释。";
pub const DEFAULT_EXTRACTION_MAX_ATTEMPTS: usize = 3;
pub const DEFAULT_EXTRACTION_RETRY_BASE_DELAY_SECS: u64 = 1;
pub const DEFAULT_EXTRACTION_REQUEST_TIMEOUT_MILLIS: u64 = 90_000;
const MAX_EXTRACTION_ATTEMPTS: usize = 5;
const MAX_EXTRACTION_RETRY_BASE_DELAY_SECS: u64 = 60;
const MIN_EXTRACTION_REQUEST_TIMEOUT_MILLIS: u64 = 1_000;
const MAX_EXTRACTION_REQUEST_TIMEOUT_MILLIS: u64 = 300_000;

/// Bounded extraction policy. Environment overrides are intentionally capped
/// so a typo cannot restore an effectively unbounded provider wait or retry
/// storm.
pub fn extraction_retry_policy_from_env() -> LlmRetryPolicy {
    extraction_retry_policy_with(|key| std::env::var(key).ok())
}

fn extraction_retry_policy_with(mut get: impl FnMut(&str) -> Option<String>) -> LlmRetryPolicy {
    let max_retries = parse_bounded_env(
        get("REFINE_EXTRACTION_MAX_ATTEMPTS"),
        DEFAULT_EXTRACTION_MAX_ATTEMPTS,
        1,
        MAX_EXTRACTION_ATTEMPTS,
    );
    let base_delay_secs = parse_bounded_env(
        get("REFINE_EXTRACTION_RETRY_BASE_DELAY_SECS"),
        DEFAULT_EXTRACTION_RETRY_BASE_DELAY_SECS,
        0,
        MAX_EXTRACTION_RETRY_BASE_DELAY_SECS,
    );
    let request_timeout_millis = parse_bounded_env(
        get("REFINE_EXTRACTION_REQUEST_TIMEOUT_MILLIS"),
        DEFAULT_EXTRACTION_REQUEST_TIMEOUT_MILLIS,
        MIN_EXTRACTION_REQUEST_TIMEOUT_MILLIS,
        MAX_EXTRACTION_REQUEST_TIMEOUT_MILLIS,
    );
    LlmRetryPolicy {
        max_retries,
        base_delay_secs,
        request_timeout_millis,
    }
}

fn parse_bounded_env<T>(raw: Option<String>, default: T, min: T, max: T) -> T
where
    T: Copy + Ord + std::str::FromStr,
{
    raw.and_then(|value| value.trim().parse::<T>().ok())
        .map(|value| value.clamp(min, max))
        .unwrap_or(default)
}

/// 提炼输入参数（供多端复用）
pub struct ItemExtractionInput<'a> {
    pub source: &'a str,
    pub title: Option<&'a str>,
    pub raw_content: &'a str,
    pub captured_at: Option<&'a str>,
    pub policy: ExtractionPolicy,
}

/// A document and all items extracted from it. Persist this aggregate through
/// `DocumentRepository::save_with_replaced_items` so foreign keys and partial
/// writes cannot diverge.
pub struct ExtractedDocument {
    pub document: Document,
    pub items: Vec<Item>,
}

/// Build and strictly extract one complete document aggregate.
pub async fn extract_document_with_strict_defaults(
    llm_client: &dyn LlmClient,
    input: &ItemExtractionInput<'_>,
    source: &Source,
) -> DomainResult<ExtractedDocument> {
    let mut document = Document::new(input.source, input.raw_content);
    if let Some(title) = input.title.filter(|value| !value.trim().is_empty()) {
        document.set_title(title);
    }
    if let Some(url) = source.url.as_deref() {
        document.set_url(url);
    } else {
        document.set_url(&format!("refine://{}/{}", input.source, document.id()));
    }
    if let Some(captured_at) = input.captured_at {
        let captured_at = chrono::DateTime::parse_from_rfc3339(captured_at)
            .map_err(|err| {
                DomainError::Validation(format!("invalid captured_at timestamp: {err}"))
            })?
            .with_timezone(&chrono::Utc);
        document.set_captured_at(captured_at);
    }

    let items =
        extract_items_with_strict_defaults(llm_client, input, source, document.id()).await?;
    Ok(ExtractedDocument { document, items })
}

/// Persist the complete extracted aggregate through the repository's atomic
/// document+items boundary and return the stable item identities.
pub async fn persist_extracted_document(
    repository: &dyn DocumentRepository,
    aggregate: &ExtractedDocument,
) -> InfraResult<Vec<String>> {
    repository
        .save_with_replaced_items(&aggregate.document, &aggregate.items)
        .await?;
    Ok(aggregate
        .items
        .iter()
        .map(|item| item.id().to_string())
        .collect())
}

/// 为提炼结果补齐来源、文档关联与兜底 content。
pub fn apply_defaults(
    items: &mut [Item],
    source: &Source,
    document_id: &DocumentId,
    raw_content: &str,
) {
    for item in items {
        item.set_source(source.clone());
        item.set_document_id(document_id.clone());
        if item.content().trim().is_empty() {
            item.set_content(raw_content);
        }
    }
}

/// 严格提炼并补齐默认值。LLM 缺失或提炼失败直接返回错误，不允许 fallback。
pub async fn extract_items_with_strict_defaults(
    llm_client: &dyn LlmClient,
    input: &ItemExtractionInput<'_>,
    source: &Source,
    document_id: &DocumentId,
) -> DomainResult<Vec<Item>> {
    let mut items =
        extract_items_with_llm(llm_client, input.raw_content, input.policy.clone()).await?;
    apply_defaults(&mut items, source, document_id, input.raw_content);
    Ok(items)
}

/// 使用 LLM 提炼 Item 列表。
pub async fn extract_items_with_llm(
    llm_client: &dyn LlmClient,
    raw_content: &str,
    policy: ExtractionPolicy,
) -> DomainResult<Vec<Item>> {
    extract_items_with_llm_policy(
        llm_client,
        raw_content,
        policy,
        extraction_retry_policy_from_env(),
    )
    .await
}

async fn extract_items_with_llm_policy(
    llm_client: &dyn LlmClient,
    raw_content: &str,
    policy: ExtractionPolicy,
    retry_policy: LlmRetryPolicy,
) -> DomainResult<Vec<Item>> {
    let conversation = Conversation::parse(raw_content)?;
    let prompt = PromptTemplate::extraction_prompt(&conversation.raw, &policy);
    let llm_response =
        protected_llm_call(llm_client, &prompt, EXTRACTION_SYSTEM_PROMPT, retry_policy)
            .await
            .map_err(|err| DomainError::Extraction(format!("LLM 调用失败: {}", err)))?;

    let extractor = Extractor::new(policy);
    let extraction = parse_extraction_with_repair(
        llm_client,
        &extractor,
        &conversation,
        &llm_response,
        retry_policy,
    )
    .await?;

    if extraction.items.is_empty() {
        return Err(DomainError::Extraction("提炼结果为空".to_string()));
    }

    Ok(extraction.items)
}

async fn protected_llm_call(
    llm_client: &dyn LlmClient,
    prompt: &str,
    system: &str,
    retry_policy: LlmRetryPolicy,
) -> InfraResult<String> {
    llm_with_retry_policy_ref(
        llm_client,
        prompt,
        system,
        retry_policy,
        LlmRetryBehavior::EXTRACTION,
        |attempt, max_attempts, delay_secs, err| {
            tracing::warn!(
                attempt,
                max_attempts,
                delay_secs,
                error = %err,
                "LLM extraction attempt failed; retrying"
            );
        },
    )
    .await
}

async fn parse_extraction_with_repair(
    llm_client: &dyn LlmClient,
    extractor: &Extractor,
    conversation: &Conversation,
    raw_response: &str,
    retry_policy: LlmRetryPolicy,
) -> DomainResult<ExtractionResult> {
    match extractor.parse_response(raw_response, conversation) {
        Ok(extraction) => Ok(extraction),
        Err(first_err) => {
            let first_message = first_err.to_string();
            tracing::warn!("首次提炼解析失败，尝试 JSON 修复重试: {}", first_message);

            let repair_prompt = build_json_repair_prompt(raw_response, &first_message);
            let repaired_response = protected_llm_call(
                llm_client,
                &repair_prompt,
                JSON_REPAIR_SYSTEM_PROMPT,
                retry_policy,
            )
            .await
            .map_err(|err| {
                DomainError::Extraction(format!("原始解析失败，且 JSON 修复请求失败: {}", err))
            })?;

            extractor
                .parse_response(&repaired_response, conversation)
                .map_err(|second_err| {
                    DomainError::Extraction(format!(
                        "原始解析失败: {}; JSON 修复后仍失败: {}",
                        first_message, second_err
                    ))
                })
        }
    }
}

fn build_json_repair_prompt(raw_response: &str, parse_error: &str) -> String {
    format!(
        r#"你会收到一段本应是 JSON 的文本，但它存在语法错误。
请将它修复为合法 JSON，并严格满足以下要求：
1) 只能输出一个 JSON 对象；
2) 顶层字段必须是 "items"；
3) "items" 必须是数组，数组元素结构为:
   {{
     "type": "knowledge|skill|snippet",
     "title": "...",
     "summary": "...",
     "content": "...",
     "tags": ["..."],
     "excerpt": "从原文逐字引用的关键片段（可选）"
   }}
4) 若无法可靠修复，请返回 {{"items":[]}}；
5) 不要输出 markdown 代码块，不要输出任何解释文字。

原始解析错误:
{}

待修复文本:
{}
"#,
        parse_error, raw_response
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{InfraError, InfraResult};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct SequenceLlmClient {
        responses: Arc<Mutex<Vec<String>>>,
    }

    struct AlwaysFailLlmClient;

    enum LlmStep {
        Return(InfraResult<String>),
        Delayed(Duration, InfraResult<String>),
    }

    struct ScriptedLlmClient {
        calls: AtomicUsize,
        steps: Mutex<VecDeque<LlmStep>>,
        systems: Mutex<Vec<Option<String>>>,
    }

    impl SequenceLlmClient {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(
                    responses.into_iter().map(ToString::to_string).collect(),
                )),
            }
        }
    }

    impl ScriptedLlmClient {
        fn new(steps: Vec<LlmStep>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                steps: Mutex::new(VecDeque::from(steps)),
                systems: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }

        fn systems(&self) -> Vec<Option<String>> {
            self.systems.lock().expect("systems lock poisoned").clone()
        }
    }

    #[async_trait]
    impl LlmClient for SequenceLlmClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            let mut guard = self
                .responses
                .lock()
                .map_err(|err| crate::error::InfraError::LlmRequest(err.to_string()))?;
            if guard.is_empty() {
                return Err(crate::error::InfraError::LlmRequest(
                    "no mock response".to_string(),
                ));
            }
            Ok(guard.remove(0))
        }
    }

    #[async_trait]
    impl LlmClient for AlwaysFailLlmClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            Err(crate::error::InfraError::LlmRequest(
                "mock failure".to_string(),
            ))
        }
    }

    #[async_trait]
    impl LlmClient for ScriptedLlmClient {
        async fn complete(&self, _prompt: &str, system: Option<&str>) -> InfraResult<String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.systems
                .lock()
                .expect("systems lock poisoned")
                .push(system.map(ToString::to_string));
            let step = self
                .steps
                .lock()
                .expect("steps lock poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    LlmStep::Return(Err(InfraError::LlmRequest(
                        "no scripted response".to_string(),
                    )))
                });
            match step {
                LlmStep::Return(result) => result,
                LlmStep::Delayed(delay, result) => {
                    tokio::time::sleep(delay).await;
                    result
                }
            }
        }
    }

    fn immediate_response(response: &str) -> LlmStep {
        LlmStep::Return(Ok(response.to_string()))
    }

    fn immediate_error(error: InfraError) -> LlmStep {
        LlmStep::Return(Err(error))
    }

    fn delayed_response(delay_millis: u64, response: &str) -> LlmStep {
        LlmStep::Delayed(
            Duration::from_millis(delay_millis),
            Ok(response.to_string()),
        )
    }

    fn fast_retry_policy(max_attempts: usize, timeout_millis: u64) -> LlmRetryPolicy {
        LlmRetryPolicy {
            max_retries: max_attempts,
            base_delay_secs: 0,
            request_timeout_millis: timeout_millis,
        }
    }

    const VALID_EXTRACTION: &str =
        r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"C","tags":[]}] }"#;
    const INVALID_EXTRACTION: &str = r#"{"items":[{"type":"knowledge","title":"broken""#;

    #[test]
    fn extraction_environment_policy_defaults_and_clamps_overrides() {
        let defaults = extraction_retry_policy_with(|_| None);
        assert_eq!(defaults.max_retries, DEFAULT_EXTRACTION_MAX_ATTEMPTS);
        assert_eq!(
            defaults.base_delay_secs,
            DEFAULT_EXTRACTION_RETRY_BASE_DELAY_SECS
        );
        assert_eq!(
            defaults.request_timeout_millis,
            DEFAULT_EXTRACTION_REQUEST_TIMEOUT_MILLIS
        );

        let bounded = extraction_retry_policy_with(|key| match key {
            "REFINE_EXTRACTION_MAX_ATTEMPTS" => Some("99".to_string()),
            "REFINE_EXTRACTION_RETRY_BASE_DELAY_SECS" => Some("999".to_string()),
            "REFINE_EXTRACTION_REQUEST_TIMEOUT_MILLIS" => Some("1".to_string()),
            _ => None,
        });
        assert_eq!(bounded.max_retries, MAX_EXTRACTION_ATTEMPTS);
        assert_eq!(
            bounded.base_delay_secs,
            MAX_EXTRACTION_RETRY_BASE_DELAY_SECS
        );
        assert_eq!(
            bounded.request_timeout_millis,
            MIN_EXTRACTION_REQUEST_TIMEOUT_MILLIS
        );
    }

    #[tokio::test]
    async fn initial_extraction_retries_transient_failure() {
        let client = ScriptedLlmClient::new(vec![
            immediate_error(InfraError::LlmHttp {
                status: 503,
                message: "temporarily unavailable".to_string(),
            }),
            immediate_response(VALID_EXTRACTION),
        ]);

        let items = extract_items_with_llm_policy(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
            fast_retry_policy(2, 50),
        )
        .await
        .expect("transient initial failure should recover");

        assert_eq!(items.len(), 1);
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn initial_extraction_timeout_is_bounded_and_retried() {
        let client = ScriptedLlmClient::new(vec![
            delayed_response(30, VALID_EXTRACTION),
            immediate_response(VALID_EXTRACTION),
        ]);

        let items = extract_items_with_llm_policy(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
            fast_retry_policy(2, 5),
        )
        .await
        .expect("timed-out initial attempt should recover");

        assert_eq!(items.len(), 1);
        assert_eq!(client.calls(), 2);
    }

    #[tokio::test]
    async fn initial_extraction_does_not_retry_deterministic_errors() {
        for error in [
            InfraError::LlmHttp {
                status: 401,
                message: "invalid api key".to_string(),
            },
            InfraError::LlmHttp {
                status: 429,
                message: "rate limited".to_string(),
            },
            InfraError::LlmRejected {
                code: "content_filter".to_string(),
                message: "blocked".to_string(),
            },
        ] {
            let client = ScriptedLlmClient::new(vec![
                immediate_error(error),
                immediate_response(VALID_EXTRACTION),
            ]);
            extract_items_with_llm_policy(
                &client,
                "Human: hello\nAssistant: world",
                ExtractionPolicy::default(),
                fast_retry_policy(3, 50),
            )
            .await
            .expect_err("auth, rate-limit, and content-policy failures must fail fast");
            assert_eq!(client.calls(), 1);
        }
    }

    #[tokio::test]
    async fn json_repair_retries_transient_failure() {
        let client = ScriptedLlmClient::new(vec![
            immediate_response(INVALID_EXTRACTION),
            immediate_error(InfraError::LlmHttp {
                status: 503,
                message: "upstream unavailable".to_string(),
            }),
            immediate_response(VALID_EXTRACTION),
        ]);

        let items = extract_items_with_llm_policy(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
            fast_retry_policy(2, 50),
        )
        .await
        .expect("transient repair failure should recover");

        assert_eq!(items.len(), 1);
        assert_eq!(client.calls(), 3);
        assert_eq!(
            client.systems(),
            vec![
                Some(EXTRACTION_SYSTEM_PROMPT.to_string()),
                Some(JSON_REPAIR_SYSTEM_PROMPT.to_string()),
                Some(JSON_REPAIR_SYSTEM_PROMPT.to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn json_repair_timeout_is_bounded_and_retried() {
        let client = ScriptedLlmClient::new(vec![
            immediate_response(INVALID_EXTRACTION),
            delayed_response(30, VALID_EXTRACTION),
            immediate_response(VALID_EXTRACTION),
        ]);

        let items = extract_items_with_llm_policy(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
            fast_retry_policy(2, 5),
        )
        .await
        .expect("timed-out repair attempt should recover");

        assert_eq!(items.len(), 1);
        assert_eq!(client.calls(), 3);
    }

    #[tokio::test]
    async fn json_repair_does_not_retry_deterministic_errors() {
        for error in [
            InfraError::LlmHttp {
                status: 429,
                message: "rate limited".to_string(),
            },
            InfraError::LlmRejected {
                code: "content_filter".to_string(),
                message: "blocked".to_string(),
            },
        ] {
            let client = ScriptedLlmClient::new(vec![
                immediate_response(INVALID_EXTRACTION),
                immediate_error(error),
                immediate_response(VALID_EXTRACTION),
            ]);

            let error = extract_items_with_llm_policy(
                &client,
                "Human: hello\nAssistant: world",
                ExtractionPolicy::default(),
                fast_retry_policy(3, 50),
            )
            .await
            .expect_err("deterministic repair failure must fail fast");

            assert!(error.to_string().contains("JSON 修复请求失败"));
            assert_eq!(client.calls(), 2);
        }
    }

    #[tokio::test]
    async fn extract_items_with_llm_works() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"C","tags":["rust"]}]}"#,
        ]);
        let items = extract_items_with_llm(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "T");
    }

    #[tokio::test]
    async fn extract_items_with_llm_repairs_json() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"broken","summary":"bad","content":"oops "quote","tags":[]}]}"#,
            r#"{"items":[{"type":"skill","title":"fixed","summary":"ok","content":"done","tags":["a"]}]}"#,
        ]);
        let items = extract_items_with_llm(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "fixed");
    }

    #[tokio::test]
    async fn extract_items_with_strict_defaults_fails_when_llm_fails() {
        let client = AlwaysFailLlmClient;
        let input = ItemExtractionInput {
            source: "chatgpt",
            title: Some("示例"),
            raw_content: "原始文本",
            captured_at: None,
            policy: ExtractionPolicy::default(),
        };
        let source = Source::new("chatgpt").with_url("https://example.com");
        let doc_id = DocumentId::new();

        let err = extract_items_with_strict_defaults(&client, &input, &source, &doc_id)
            .await
            .expect_err("strict mode should not fallback");
        assert!(err.to_string().contains("LLM 调用失败"));
    }

    #[tokio::test]
    async fn extract_items_with_strict_defaults_applies_source_and_content() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"","tags":[]}]}"#,
        ]);
        let input = ItemExtractionInput {
            source: "chatgpt",
            title: Some("示例"),
            raw_content: "原始文本",
            captured_at: None,
            policy: ExtractionPolicy::default(),
        };
        let source = Source::new("chatgpt").with_url("https://example.com");
        let doc_id = DocumentId::new();

        let items = extract_items_with_strict_defaults(&client, &input, &source, &doc_id)
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].source().map(|v| v.platform.as_str()),
            Some("chatgpt")
        );
        assert_eq!(items[0].content(), "原始文本");
        assert_eq!(
            items[0].document_id().map(|id| id.as_str()),
            Some(doc_id.as_str())
        );
    }

    #[tokio::test]
    async fn extracted_document_uses_one_identity_for_document_and_items() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"C","tags":[]}]}"#,
        ]);
        let input = ItemExtractionInput {
            source: "cli",
            title: Some("Imported conversation"),
            raw_content: "Human: hello\nAssistant: world",
            captured_at: Some("2026-08-13T00:00:00Z"),
            policy: ExtractionPolicy::default(),
        };
        let source = Source::new("cli").with_url("stdin://conversation");

        let aggregate = extract_document_with_strict_defaults(&client, &input, &source)
            .await
            .unwrap();

        assert_eq!(aggregate.document.title(), Some("Imported conversation"));
        assert_eq!(aggregate.document.url(), "stdin://conversation");
        assert_eq!(aggregate.items.len(), 1);
        assert_eq!(
            aggregate.items[0].document_id(),
            Some(aggregate.document.id())
        );
    }

    #[tokio::test]
    async fn extracted_document_persists_document_and_items_atomically() {
        use crate::infra::SqliteStore;
        use crate::knowledge::{DocumentRepository, ItemRepository};
        use tempfile::tempdir;

        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"C","tags":[]}]}"#,
        ]);
        let input = ItemExtractionInput {
            source: "cli",
            title: None,
            raw_content: "Human: hello\nAssistant: world",
            captured_at: None,
            policy: ExtractionPolicy::default(),
        };
        let aggregate = extract_document_with_strict_defaults(&client, &input, &Source::new("cli"))
            .await
            .unwrap();
        let temp = tempdir().unwrap();
        let store = SqliteStore::open(&temp.path().join("refine.db")).unwrap();

        let ids = persist_extracted_document(&store, &aggregate)
            .await
            .unwrap();

        assert_eq!(ids.len(), 1);
        assert!(
            DocumentRepository::find_by_id(&store, aggregate.document.id())
                .await
                .unwrap()
                .is_some()
        );
        let persisted = ItemRepository::find_by_document_id(&store, aggregate.document.id())
            .await
            .unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].id().as_str(), ids[0]);
    }

    #[tokio::test]
    async fn aggregate_persistence_rolls_back_document_and_prior_items_on_failure() {
        use crate::infra::SqliteStore;
        use crate::knowledge::{DocumentRepository, ItemRepository};
        use tempfile::tempdir;

        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"first","summary":"S","content":"C","tags":[]},{"type":"skill","title":"second","summary":"S","content":"C","tags":[]}]}"#,
        ]);
        let input = ItemExtractionInput {
            source: "cli",
            title: None,
            raw_content: "Human: hello\nAssistant: world",
            captured_at: None,
            policy: ExtractionPolicy::default(),
        };
        let mut aggregate =
            extract_document_with_strict_defaults(&client, &input, &Source::new("cli"))
                .await
                .unwrap();
        aggregate.items[1].set_document_id(DocumentId::from("missing-document"));
        let temp = tempdir().unwrap();
        let store = SqliteStore::open(&temp.path().join("refine.db")).unwrap();

        let err = persist_extracted_document(&store, &aggregate)
            .await
            .expect_err("the second item must violate its document foreign key");

        assert!(err.to_string().contains("FOREIGN KEY"));
        assert!(
            DocumentRepository::find_by_id(&store, aggregate.document.id())
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(ItemRepository::count_items(&store, None).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn repeated_url_reuses_canonical_document_identity() {
        use crate::infra::SqliteStore;
        use crate::knowledge::{DocumentRepository, ItemRepository};
        use tempfile::tempdir;

        let input = ItemExtractionInput {
            source: "browser",
            title: None,
            raw_content: "Human: hello\nAssistant: world",
            captured_at: Some("2020-01-02T03:04:05Z"),
            policy: ExtractionPolicy::default(),
        };
        let source = Source::new("browser").with_url("https://same.example/conversation");
        let first = extract_document_with_strict_defaults(
            &SequenceLlmClient::new(vec![r#"{"items":[{"type":"knowledge","title":"first","summary":"S","content":"C","tags":[]}]}"#]),
            &input,
            &source,
        )
        .await
        .unwrap();
        let second = extract_document_with_strict_defaults(
            &SequenceLlmClient::new(vec![r#"{"items":[{"type":"knowledge","title":"second","summary":"S","content":"C","tags":[]}]}"#]),
            &input,
            &source,
        )
        .await
        .unwrap();
        assert_ne!(first.document.id(), second.document.id());
        let temp = tempdir().unwrap();
        let store = SqliteStore::open(&temp.path().join("refine.db")).unwrap();

        persist_extracted_document(&store, &first).await.unwrap();
        persist_extracted_document(&store, &second).await.unwrap();

        let canonical = DocumentRepository::find_by_url(&store, source.url.as_deref().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(canonical.id(), first.document.id());
        assert_eq!(
            canonical.captured_at().to_rfc3339(),
            "2020-01-02T03:04:05+00:00"
        );
        let items = ItemRepository::find_by_document_id(&store, canonical.id())
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "second");
    }

    #[tokio::test]
    async fn concurrent_same_url_persistence_converges_without_foreign_key_errors() {
        use crate::infra::SqliteStore;
        use crate::knowledge::{DocumentRepository, ItemRepository};
        use tempfile::tempdir;

        let input = ItemExtractionInput {
            source: "browser",
            title: None,
            raw_content: "Human: hello\nAssistant: world",
            captured_at: None,
            policy: ExtractionPolicy::default(),
        };
        let source = Source::new("browser").with_url("https://same.example/concurrent");
        let first = extract_document_with_strict_defaults(
            &SequenceLlmClient::new(vec![r#"{"items":[{"type":"knowledge","title":"first","summary":"S","content":"C","tags":[]}]}"#]),
            &input,
            &source,
        )
        .await
        .unwrap();
        let second = extract_document_with_strict_defaults(
            &SequenceLlmClient::new(vec![r#"{"items":[{"type":"knowledge","title":"second","summary":"S","content":"C","tags":[]}]}"#]),
            &input,
            &source,
        )
        .await
        .unwrap();
        let temp = tempdir().unwrap();
        let store = SqliteStore::open(&temp.path().join("refine.db")).unwrap();

        let (first_result, second_result) = tokio::join!(
            persist_extracted_document(&store, &first),
            persist_extracted_document(&store, &second)
        );
        first_result.unwrap();
        second_result.unwrap();

        assert_eq!(DocumentRepository::count(&store).await.unwrap(), 1);
        assert_eq!(ItemRepository::count_items(&store, None).await.unwrap(), 1);
    }
}
