use super::provenance::replace_session_mode_tags;
use super::quarantine::{record_key as quarantine_key, QuarantineStore};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::error::InfraError;
use refine_core::infra::{
    llm_with_retry_policy_for, LlmClient, LlmRetryPolicy, DEFAULT_RETRY_BASE_DELAY_SECS,
};
use refine_core::knowledge::{
    assign_session_observation_ids, Document, DocumentRepository, RestoreDocumentParams,
    SessionProjectionMetadata,
};
use refine_core::session::{
    build_facet_prompt, facets_to_items_with_mode_and_identity, parse_facet_response,
    session_projection_evidence, validate_facet_evidence, SessionMode, SessionSource,
    SourceMessageReference, FACET_SYSTEM_PROMPT,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

const DEFAULT_CONCURRENCY: usize = 1;
const DEFAULT_FACET_PARSE_ATTEMPTS: usize = 2;

// Separate from the 25,000-byte soft chunk target and the process token budget.
// Count rendered UTF-8 prompt/system text plus a fixed framing allowance; this
// is a local input contract, not exact provider tokens or HTTP-body bytes.
const FACET_REQUEST_MAX_BYTES: usize = 65_536;
const FACET_REQUEST_FRAMING_BYTES: usize = 1_024;

fn checked_facet_prompt(content: &str, stage: &'static str) -> Result<String, InfraError> {
    let prompt = build_facet_prompt(content);
    let request_bytes = prompt
        .len()
        .saturating_add(FACET_SYSTEM_PROMPT.len())
        .saturating_add(FACET_REQUEST_FRAMING_BYTES);
    if request_bytes > FACET_REQUEST_MAX_BYTES {
        return Err(InfraError::FacetRequestTooLarge {
            stage,
            request_bytes,
            limit_bytes: FACET_REQUEST_MAX_BYTES,
        });
    }
    Ok(prompt)
}

fn concurrency() -> usize {
    std::env::var("REFINE_INGEST_CONCURRENCY")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&value| value >= 1)
        .unwrap_or(DEFAULT_CONCURRENCY)
}

pub(super) struct PendingSession {
    pub(super) idx: usize,
    pub(super) total: usize,
    pub(super) url: String,
    pub(super) source: SessionSource,
    pub(super) project: Option<String>,
    pub(super) project_identity: Option<String>,
    pub(super) mode: SessionMode,
    pub(super) captured_at: DateTime<Utc>,
    pub(super) has_embedded_timestamp: bool,
    pub(super) raw_content: String,
    pub(super) facet_content: Option<String>,
    pub(super) source_messages: Vec<SourceMessageReference>,
    pub(super) source_version: Option<String>,
    pub(super) needs_chunk: bool,
    pub(super) chunks: Vec<String>,
    pub(super) existing_document: Option<Document>,
    pub(super) legacy_documents_to_delete: Vec<refine_core::knowledge::DocumentId>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn process_pending_sessions(
    mut pending: Vec<PendingSession>,
    skipped_dup: usize,
    skipped_filter: usize,
    stale_refresh: usize,
    mut skipped_quarantined: usize,
    mut selected_identities: HashSet<String>,
    dry_run: bool,
    retry_quarantined: bool,
    quarantine: Option<QuarantineStore>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    if dry_run {
        for session in &pending {
            println!(
                "  [dry-run] {} | {} chars | remem",
                session.url,
                session.raw_content.chars().count(),
            );
        }
        println!(
            "\n[dry-run] 最终选择 {}, 跳过重复 {}, 过滤 {}, 隔离跳过 {}, 刷新过期 {}",
            pending.len(),
            skipped_dup,
            skipped_filter,
            skipped_quarantined,
            stale_refresh
        );
        return Ok(());
    }

    let mut quarantine = match quarantine {
        Some(quarantine) => quarantine,
        None => QuarantineStore::load()?,
    };
    selected_identities.extend(
        pending
            .iter()
            .map(|session| quarantine_key(&session.url, session.source_version.as_deref())),
    );
    if !retry_quarantined {
        pending.retain(|session| {
            if quarantine.contains(&session.url, session.source_version.as_deref()) {
                skipped_quarantined += 1;
                false
            } else {
                true
            }
        });
    }

    let concurrency = concurrency();
    println!(
        "待处理 {} 个会话（跳过重复 {}, 过滤 {}, 刷新过期 {}, 隔离跳过 {}），{} 路并发...\n",
        pending.len(),
        skipped_dup,
        skipped_filter,
        stale_refresh,
        skipped_quarantined,
        concurrency,
    );
    if pending.is_empty() {
        let selected_quarantine_count = quarantine.count_matching(&selected_identities);
        if selected_quarantine_count > 0 {
            anyhow::bail!(
                "本次选择中仍有 {} 个会话处于隔离状态；队列: {}；确认上游策略已修复后使用 --retry-quarantined",
                selected_quarantine_count,
                quarantine.path().display()
            );
        }
        if quarantine.len() > 0 {
            println!(
                "全部已处理完毕；隔离队列另有 {} 个不在本次选择范围内的记录。",
                quarantine.len()
            );
        } else {
            println!("全部已处理完毕，隔离队列为空。");
        }
        return Ok(());
    }

    let client = llm_client.ok_or_else(|| anyhow::anyhow!("非 dry-run 模式需要 LLM API Key"))?;
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let processed = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total_items = Arc::new(AtomicUsize::new(0));
    let quota_hit = Arc::new(AtomicBool::new(false));
    let succeeded_sessions = Arc::new(Mutex::new(HashSet::<(String, Option<String>)>::new()));
    let rejected_sessions = Arc::new(Mutex::new(
        Vec::<(String, Option<String>, String, String)>::new(),
    ));
    let mut handles = Vec::new();

    for session in pending {
        let permit_pool = semaphore.clone();
        let client = client.clone();
        let doc_store = doc_store.clone();
        let processed = processed.clone();
        let failed = failed.clone();
        let total_items = total_items.clone();
        let quota_hit = quota_hit.clone();
        let succeeded_sessions = succeeded_sessions.clone();
        let rejected_sessions = rejected_sessions.clone();
        handles.push(tokio::spawn(async move {
            let Ok(_permit) = permit_pool.acquire().await else {
                eprintln!(
                    "  ✗ [{}/{}] 失败: ingest semaphore closed",
                    session.idx + 1,
                    session.total
                );
                failed.fetch_add(1, Ordering::Relaxed);
                return;
            };
            match process_single_session(&session, &client, &doc_store, &quota_hit).await {
                Ok(item_count) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    total_items.fetch_add(item_count, Ordering::Relaxed);
                    match succeeded_sessions.lock() {
                        Ok(mut succeeded) => {
                            succeeded.insert((session.url.clone(), session.source_version.clone()));
                        }
                        Err(_) => {
                            eprintln!(
                                "  ✗ [{}/{}] 失败: succeeded session lock poisoned",
                                session.idx + 1,
                                session.total
                            );
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(error) => {
                    if let Some((code, message)) = content_rejection(&error) {
                        eprintln!(
                            "  ⛔ [{}/{}] 隔离: {} ({})",
                            session.idx + 1,
                            session.total,
                            code,
                            session.url
                        );
                        if code == "facet_request_too_large" {
                            // The typed size error contains only stages and counts.
                            eprintln!("    {}", message);
                        }
                        match rejected_sessions.lock() {
                            Ok(mut rejected) => rejected.push((
                                session.url.clone(),
                                session.source_version.clone(),
                                code,
                                message,
                            )),
                            Err(_) => {
                                eprintln!(
                                    "  ✗ [{}/{}] 失败: rejected session lock poisoned",
                                    session.idx + 1,
                                    session.total
                                );
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    } else {
                        eprintln!(
                            "  ✗ [{}/{}] 失败: {}",
                            session.idx + 1,
                            session.total,
                            error
                        );
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }));
    }

    for handle in handles {
        handle.await.context("session ingest worker panicked")?;
    }

    let processed = processed.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    let total_items = total_items.load(Ordering::Relaxed);
    let succeeded = succeeded_sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("succeeded session lock poisoned"))?;
    for (url, _) in succeeded.iter() {
        quarantine.resolve(url);
    }
    drop(succeeded);
    let rejected = rejected_sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("rejected session lock poisoned"))?;
    for (url, source_version, code, message) in rejected.iter() {
        quarantine.record(url, source_version.as_deref(), code, message);
    }
    let rejected_count = rejected.len();
    drop(rejected);
    quarantine.save_if_dirty()?;
    let quarantine_count = quarantine.len();
    let selected_quarantine_count = quarantine.count_matching(&selected_identities);
    println!(
        "\n完成: 处理 {}, 跳过重复 {}, 过滤 {}, 刷新过期 {}, 失败 {}, 新增隔离 {}, 本次相关隔离 {}, 隔离总数 {}, 生成 {} 条观测",
        processed,
        skipped_dup,
        skipped_filter,
        stale_refresh,
        failed,
        rejected_count,
        selected_quarantine_count,
        quarantine_count,
        total_items
    );
    if failed > 0 || selected_quarantine_count > 0 {
        if failed > 0 {
            eprintln!("提示: 瞬态失败会在下次运行续传");
        }
        if selected_quarantine_count > 0 {
            eprintln!(
                "提示: 本次选择中的 {} 个确定性拒绝已隔离，不会自动重试；队列: {}",
                selected_quarantine_count,
                quarantine.path().display()
            );
        }
        anyhow::bail!(
            "摄入不完整: 瞬态失败 {}, 本次相关隔离 {}",
            failed,
            selected_quarantine_count
        );
    }
    Ok(())
}

pub(super) async fn process_single_session(
    session: &PendingSession,
    client: &Arc<dyn LlmClient>,
    doc_store: &Arc<dyn DocumentRepository>,
    quota_hit: &Arc<AtomicBool>,
) -> Result<usize> {
    let mut reduced_sources = Vec::new();
    let content = if session.needs_chunk {
        let total_chunks = session.chunks.len();
        // Reject an indivisible oversized message before spending on any chunk.
        // The common call boundary checks the same contract again for reduction
        // and regeneration; no source text or identity is changed.
        for chunk in &session.chunks {
            checked_facet_prompt(chunk, "chunk extraction")?;
        }
        let mut summaries = Vec::with_capacity(total_chunks);
        let mut referenced_ids = HashSet::new();
        for (idx, chunk) in session.chunks.iter().enumerate() {
            let facets = extract_and_parse_facets_with_retry(
                chunk,
                client,
                quota_hit,
                &session.source_messages,
                "chunk extraction",
            )
            .await
            .with_context(|| {
                format!(
                    "分块 {}/{} 提取失败，整个 session 视为失败以避免数据缺失",
                    idx + 1,
                    total_chunks
                )
            })?;
            referenced_ids.extend(
                facets
                    .evidence
                    .iter()
                    .flat_map(|entry| entry.message_ids.iter().copied()),
            );
            summaries.push(serde_json::to_string(&facets)?);
        }
        reduced_sources.extend(
            session
                .source_messages
                .iter()
                .filter(|message| referenced_ids.contains(&message.id))
                .cloned(),
        );
        summaries.join("\n\n---\n\n")
    } else {
        session
            .facet_content
            .as_ref()
            .unwrap_or(&session.raw_content)
            .clone()
    };

    let evidence_sources = if session.needs_chunk {
        &reduced_sources
    } else {
        &session.source_messages
    };
    let stage = if session.needs_chunk {
        "final reduction"
    } else {
        "session extraction"
    };
    let facet_result =
        extract_and_parse_facets_with_retry(&content, client, quota_hit, evidence_sources, stage)
            .await;
    let facet_response = facet_result.map_err(|error| {
        if session.needs_chunk
            && matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { .. }))
        {
            error.context(format!(
                "最终汇总失败；已完成 {} 个分块提炼（实际 provider attempts 保留在用量账本中），未发布会话",
                session.chunks.len()
            ))
        } else {
            error
        }
    })?;
    let document = build_session_document(session, &facet_response.session_summary);
    let mut items = facets_to_items_with_mode_and_identity(
        &facet_response,
        document.id(),
        session.project.as_deref(),
        session.project_identity.as_deref(),
        session.mode,
    );
    assign_session_observation_ids(&mut items, &session.url)?;
    let evidence = session_projection_evidence(&facet_response, &session.source_messages, &items)
        .map_err(anyhow::Error::msg)?;
    let item_count = items.len();
    for legacy_document_id in &session.legacy_documents_to_delete {
        let mut legacy_items = doc_store
            .find_items_by_document_id(legacy_document_id)
            .await
            .context("加载待迁移旧会话 Items 失败")?;
        replace_session_mode_tags(&mut legacy_items, session.mode)?;
        for item in &mut legacy_items {
            item.set_document_id(document.id().clone());
        }
        items.extend(legacy_items);
    }
    doc_store
        .save_session_projection(
            &document,
            &items,
            &session.legacy_documents_to_delete,
            &session.legacy_documents_to_delete,
            &SessionProjectionMetadata {
                recipe_id: refine_core::session::facet_recipe_identity(&client.cache_identity()),
                evidence,
            },
        )
        .await
        .context("保存 Document/Items 并清理旧会话失败")?;

    println!(
        "  + [{}/{}] {} | {} items",
        session.idx + 1,
        session.total,
        facet_response.session_summary,
        item_count,
    );
    Ok(item_count)
}

pub(super) fn content_rejection(error: &anyhow::Error) -> Option<(String, String)> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<InfraError>()
            .and_then(|infra_error| match infra_error {
                InfraError::LlmRejected { code, message } => Some((code.clone(), message.clone())),
                InfraError::FacetRequestTooLarge { .. } => {
                    Some(("facet_request_too_large".to_string(), format!("{error:#}")))
                }
                _ => None,
            })
    })
}

fn build_session_document(session: &PendingSession, title: &str) -> Document {
    if let Some(existing_document) = &session.existing_document {
        let captured_at = if session.has_embedded_timestamp {
            session.captured_at
        } else {
            existing_document.captured_at()
        };

        return Document::restore(RestoreDocumentParams {
            id: existing_document.id().clone(),
            title: Some(title.to_string()),
            raw_content: String::new(),
            source: session.source.as_str().to_string(),
            url: session.url.clone(),
            source_version: session.source_version.clone(),
            captured_at,
            created_at: existing_document.created_at(),
            updated_at: Utc::now(),
        });
    }

    let mut document = Document::new(session.source.as_str(), "");
    document.set_title(title);
    document.set_url(&session.url);
    document.set_source_version(session.source_version.as_deref());
    document.set_captured_at(session.captured_at);
    document
}

pub(super) async fn llm_call_with_retry(
    client: &Arc<dyn LlmClient>,
    content: &str,
    quota_hit: &Arc<AtomicBool>,
    stage: &'static str,
) -> Result<String> {
    if quota_hit.load(Ordering::Relaxed) {
        return Err(anyhow::anyhow!("LLM 配额已耗尽或本次运行预算已耗尽，跳过"));
    }

    let prompt = checked_facet_prompt(content, stage)?;
    let result = llm_with_retry_policy_for(
        client,
        "ingest.session.facets",
        &prompt,
        FACET_SYSTEM_PROMPT,
        LlmRetryPolicy::default(),
        |attempt, max_retries, delay_secs, error| {
            let message = error.to_string();
            let preview = log_preview(&message, 80);
            eprintln!(
                "    ⏳ 重试 ({}/{}) 等待 {}s: {}",
                attempt, max_retries, delay_secs, preview,
            );
        },
    )
    .await;
    finish_llm_call(result, quota_hit)
}

pub(super) fn finish_llm_call(
    result: std::result::Result<String, InfraError>,
    quota_hit: &Arc<AtomicBool>,
) -> Result<String> {
    match result {
        Ok(response) => Ok(response),
        Err(InfraError::RateLimited { retry_after_secs }) => {
            quota_hit.store(true, Ordering::Relaxed);
            Err(anyhow::anyhow!(
                "LLM 配额已耗尽 (retry_after: {:?}s)",
                retry_after_secs
            ))
        }
        Err(error @ InfraError::LlmBudgetExceeded { .. }) => {
            quota_hit.store(true, Ordering::Relaxed);
            Err(anyhow::Error::new(error))
        }
        Err(error) => Err(anyhow::Error::new(error)),
    }
}

async fn extract_and_parse_facets_with_retry(
    content: &str,
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
    source_messages: &[SourceMessageReference],
    stage: &'static str,
) -> Result<refine_core::session::FacetResponse> {
    extract_and_validate_facets_with_retry_policy(
        content,
        client,
        quota_hit,
        source_messages,
        DEFAULT_FACET_PARSE_ATTEMPTS,
        DEFAULT_RETRY_BASE_DELAY_SECS,
        stage,
    )
    .await
}

#[cfg(test)]
pub(super) async fn extract_and_parse_facets_with_retry_policy(
    content: &str,
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
    max_retries: usize,
    base_delay_secs: u64,
) -> Result<refine_core::session::FacetResponse> {
    extract_and_validate_facets_with_retry_policy(
        content,
        client,
        quota_hit,
        &[],
        max_retries,
        base_delay_secs,
        "session extraction",
    )
    .await
}

async fn extract_and_validate_facets_with_retry_policy(
    content: &str,
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
    source_messages: &[SourceMessageReference],
    max_retries: usize,
    base_delay_secs: u64,
    stage: &'static str,
) -> Result<refine_core::session::FacetResponse> {
    let max_retries = max_retries.max(1);

    for attempt in 0..max_retries {
        let response = llm_call_with_retry(client, content, quota_hit, stage).await?;
        match parse_facet_response(&response).and_then(|facets| {
            validate_facet_evidence(&facets, source_messages)?;
            Ok(facets)
        }) {
            Ok(facets) => return Ok(facets),
            Err(error) if attempt == max_retries - 1 => return Err(anyhow::anyhow!(error)),
            Err(error) => {
                let delay_secs = ingest_retry_delay_secs(base_delay_secs, attempt);
                eprintln!(
                    "    ⏳ 解析重试 ({}/{}) 等待 {}s: {}",
                    attempt + 1,
                    max_retries,
                    delay_secs,
                    log_preview(&error, 80),
                );
                if delay_secs > 0 {
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }

    unreachable!("parse retry loop always returns on success or failure")
}

fn ingest_retry_delay_secs(base_delay_secs: u64, attempt: usize) -> u64 {
    let backoff_factor = 1u64.checked_shl(attempt as u32).unwrap_or(u64::MAX);
    base_delay_secs.saturating_mul(backoff_factor)
}

#[cfg(test)]
mod retry_default_tests {
    use super::DEFAULT_FACET_PARSE_ATTEMPTS;

    #[test]
    fn facet_parse_allows_one_regeneration() {
        assert_eq!(DEFAULT_FACET_PARSE_ATTEMPTS, 2);
    }
}

#[cfg(test)]
mod request_limit_tests {
    use super::*;
    use async_trait::async_trait;
    use refine_core::error::InfraResult;
    use refine_core::infra::{LlmRunBudget, SqliteStore};
    use refine_core::knowledge::{Item, ItemRepository};
    use refine_core::session::{
        chunk_session, needs_chunking, MessageProvenance, MessageRole, Session, SessionMessage,
        SessionMeta,
    };
    use std::collections::VecDeque;
    use std::path::PathBuf;

    struct RecordingClient {
        requests: Mutex<Vec<(String, String)>>,
        responses: Mutex<VecDeque<String>>,
        ledger: Option<PathBuf>,
        budget: LlmRunBudget,
        transient_first: AtomicBool,
    }

    impl RecordingClient {
        fn new(responses: Vec<String>, ledger: Option<PathBuf>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
                ledger,
                budget: LlmRunBudget::default(),
                transient_first: AtomicBool::new(false),
            }
        }

        fn calls(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl LlmClient for RecordingClient {
        async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
            self.requests
                .lock()
                .unwrap()
                .push((prompt.to_string(), system.unwrap_or_default().to_string()));
            if self.transient_first.swap(false, Ordering::Relaxed) {
                return Err(InfraError::LlmTransport("synthetic disconnect".to_string()));
            }
            Ok(self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| valid_response("synthetic summary")))
        }

        fn usage_ledger_path(&self) -> InfraResult<Option<PathBuf>> {
            Ok(self.ledger.clone())
        }

        fn run_budget(&self) -> Option<&LlmRunBudget> {
            Some(&self.budget)
        }
    }

    fn valid_response(summary: &str) -> String {
        serde_json::json!({
            "session_summary": summary,
            "cognitive_level": "unknown",
            "collaboration_mode": "unknown"
        })
        .to_string()
    }

    fn synthetic_session(messages: Vec<SessionMessage>) -> Session {
        Session {
            source: SessionSource::RememRaw,
            file_path: PathBuf::from("/synthetic/session"),
            messages,
            meta: SessionMeta::default(),
        }
    }

    fn message(content: String) -> SessionMessage {
        SessionMessage {
            role: MessageRole::User,
            content,
            provenance: None,
        }
    }

    fn pending(session: &Session, url: &str) -> PendingSession {
        let needs_chunk = needs_chunking(session);
        PendingSession {
            idx: 0,
            total: 2,
            url: url.to_string(),
            source: session.source.clone(),
            project: None,
            project_identity: None,
            mode: SessionMode::Unknown,
            captured_at: Utc::now(),
            has_embedded_timestamp: false,
            raw_content: session.to_document_content(),
            facet_content: Some(session.to_facet_content()),
            source_messages: session.source_message_references(),
            source_version: Some("synthetic-next-version".to_string()),
            needs_chunk,
            chunks: if needs_chunk {
                chunk_session(session)
                    .into_iter()
                    .map(|c| c.content)
                    .collect()
            } else {
                Vec::new()
            },
            existing_document: None,
            legacy_documents_to_delete: Vec::new(),
        }
    }

    #[tokio::test]
    async fn facet_request_exact_limit_counts_template_system_roles_and_provenance() {
        let overhead =
            build_facet_prompt("").len() + FACET_SYSTEM_PROMPT.len() + FACET_REQUEST_FRAMING_BYTES;
        for role in [
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::System,
        ] {
            for with_provenance in [false, true] {
                for multibyte in [false, true] {
                    let mut source = SessionMessage {
                        role: role.clone(),
                        content: String::new(),
                        provenance: with_provenance.then(|| MessageProvenance {
                            id: 42,
                            event_time: "2026-10-07T03:00:00Z".parse().unwrap(),
                        }),
                    };
                    let empty = synthetic_session(vec![source.clone()]).to_facet_content();
                    let remaining = FACET_REQUEST_MAX_BYTES - overhead - empty.len();
                    source.content = if multibyte {
                        format!(
                            "{}{}",
                            "你".repeat(remaining / 3),
                            "x".repeat(remaining % 3)
                        )
                    } else {
                        "x".repeat(remaining)
                    };
                    let original = synthetic_session(vec![source.clone()]);
                    let content = original.to_facet_content();
                    let chunks = chunk_session(&original);
                    assert_eq!(chunks.len(), 1, "whole message must not split");
                    assert_eq!(chunks[0].content, content);
                    let client = Arc::new(RecordingClient::new(Vec::new(), None));
                    let client_dyn: Arc<dyn LlmClient> = client.clone();
                    let quota = Arc::new(AtomicBool::new(false));
                    llm_call_with_retry(&client_dyn, &content, &quota, "session extraction")
                        .await
                        .unwrap();
                    {
                        let requests = client.requests.lock().unwrap();
                        assert_eq!(requests[0].0, build_facet_prompt(&content));
                        assert_eq!(requests[0].1, FACET_SYSTEM_PROMPT);
                        assert_eq!(
                            requests[0].0.len() + requests[0].1.len() + FACET_REQUEST_FRAMING_BYTES,
                            FACET_REQUEST_MAX_BYTES
                        );
                    }

                    source.content.push('x');
                    let oversized = synthetic_session(vec![source]).to_facet_content();
                    let error = extract_and_parse_facets_with_retry_policy(
                        &oversized,
                        &client_dyn,
                        &quota,
                        2,
                        0,
                    )
                    .await
                    .unwrap_err();
                    assert!(
                        matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { request_bytes, limit_bytes, stage: "session extraction" }) if *request_bytes == FACET_REQUEST_MAX_BYTES + 1 && *limit_bytes == FACET_REQUEST_MAX_BYTES)
                    );
                    assert_eq!(
                        client.calls(),
                        1,
                        "L + 1 must never reach provider or regenerate"
                    );
                    assert!(!quota.load(Ordering::Relaxed));
                    assert!(!error.to_string().contains(&content));
                }
            }
        }
    }

    #[tokio::test]
    async fn facet_request_soft_target_and_activation_edges_remain_accepted() {
        let client = Arc::new(RecordingClient::new(Vec::new(), None));
        let client_dyn: Arc<dyn LlmClient> = client.clone();
        let quota = Arc::new(AtomicBool::new(false));
        for bytes in [25_000, 25_001, 29_999, 30_000, 30_001] {
            let session = synthetic_session(vec![message("x".repeat(bytes - "User: \n".len()))]);
            assert_eq!(session.to_facet_content().len(), bytes);
            assert_eq!(needs_chunking(&session), bytes > 30_000);
            assert_eq!(
                chunk_session(&session)[0].content,
                session.to_facet_content()
            );
            llm_call_with_retry(
                &client_dyn,
                &session.to_facet_content(),
                &quota,
                "session extraction",
            )
            .await
            .unwrap();
        }
        assert_eq!(client.calls(), 5);
    }

    #[tokio::test]
    async fn facet_request_regeneration_and_provider_retry_keep_the_same_bounded_input() {
        let overhead =
            build_facet_prompt("").len() + FACET_SYSTEM_PROMPT.len() + FACET_REQUEST_FRAMING_BYTES;
        let content = "x".repeat(FACET_REQUEST_MAX_BYTES - overhead);
        let client = Arc::new(RecordingClient::new(
            vec![
                "synthetic invalid JSON".to_string(),
                valid_response("regenerated"),
            ],
            None,
        ));
        client.transient_first.store(true, Ordering::Relaxed);
        let quota = Arc::new(AtomicBool::new(false));
        let result = extract_and_parse_facets_with_retry_policy(
            &content,
            &(client.clone() as Arc<dyn LlmClient>),
            &quota,
            2,
            0,
        )
        .await
        .unwrap();
        assert_eq!(result.session_summary, "regenerated");
        assert_eq!(client.calls(), 3);
        for (prompt, system) in client.requests.lock().unwrap().iter() {
            assert_eq!(prompt, &build_facet_prompt(&content));
            assert_eq!(system, FACET_SYSTEM_PROMPT);
            assert_eq!(
                prompt.len() + system.len() + FACET_REQUEST_FRAMING_BYTES,
                FACET_REQUEST_MAX_BYTES
            );
        }
    }

    #[tokio::test]
    async fn facet_request_later_oversized_chunk_preflight_spends_nothing_and_preserves_projection()
    {
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let mut old = Document::new("remem-raw-session", "");
        old.set_url("remem://synthetic/oversized");
        old.set_title("prior summary");
        old.set_source_version(Some("prior-version"));
        DocumentRepository::save(store.as_ref(), &old)
            .await
            .unwrap();
        let mut item = Item::new_observation("prior decision", "prior reason");
        item.set_document_id(old.id().clone());
        ItemRepository::save(store.as_ref(), &item).await.unwrap();
        let session = synthetic_session(vec![
            message("x".repeat(25_000)),
            message("private-synthetic-marker".repeat(3_000)),
        ]);
        let mut pending = pending(&session, old.url());
        assert_eq!(pending.chunks.len(), 2);
        pending.existing_document = Some(old.clone());
        let client = Arc::new(RecordingClient::new(Vec::new(), None));
        let quota = Arc::new(AtomicBool::new(false));
        let doc_store: Arc<dyn DocumentRepository> = store.clone();
        let error = process_single_session(
            &pending,
            &(client.clone() as Arc<dyn LlmClient>),
            &doc_store,
            &quota,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<InfraError>(),
            Some(InfraError::FacetRequestTooLarge {
                stage: "chunk extraction",
                ..
            })
        ));
        assert_eq!(client.calls(), 0);
        assert!(!quota.load(Ordering::Relaxed));
        let saved = store.find_by_url(old.url()).await.unwrap().unwrap();
        assert_eq!(saved.source_version(), Some("prior-version"));
        assert_eq!(saved.title(), old.title());
        assert_eq!(
            store.find_items_by_document_id(old.id()).await.unwrap()[0].id(),
            item.id()
        );
        assert!(store
            .find_session_projection_versions()
            .await
            .unwrap()
            .is_empty());
        assert!(store
            .find_session_projection_history(old.id(), 10)
            .await
            .unwrap()
            .is_empty());
        let rejection = content_rejection(&error.context("synthetic chunk preflight")).unwrap();
        assert_eq!(rejection.0, "facet_request_too_large");
        assert!(!rejection.1.contains("private-synthetic-marker"));
    }

    #[tokio::test]
    async fn facet_request_final_reduction_rejects_without_partial_publish_and_keeps_spent_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = tmp.path().join("usage.jsonl");
        let response = valid_response(&"synthetic-private-summary".repeat(1_000));
        let normalized = serde_json::to_string(&parse_facet_response(&response).unwrap()).unwrap();
        assert!(checked_facet_prompt(&normalized, "chunk extraction").is_ok());
        let reduction = [normalized.clone(), normalized.clone(), normalized].join("\n\n---\n\n");
        let expected_bytes = build_facet_prompt(&reduction).len()
            + FACET_SYSTEM_PROMPT.len()
            + FACET_REQUEST_FRAMING_BYTES;
        assert!(expected_bytes > FACET_REQUEST_MAX_BYTES);
        let client = Arc::new(RecordingClient::new(
            vec![response; 3],
            Some(ledger.clone()),
        ));
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let doc_store: Arc<dyn DocumentRepository> = store.clone();
        let session = synthetic_session((0..3).map(|_| message("x".repeat(20_000))).collect());
        let pending = pending(&session, "remem://synthetic/reduction");
        assert_eq!(pending.chunks.len(), 3);
        let quota = Arc::new(AtomicBool::new(false));
        let error = process_single_session(
            &pending,
            &(client.clone() as Arc<dyn LlmClient>),
            &doc_store,
            &quota,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { stage: "final reduction", request_bytes, .. }) if *request_bytes == expected_bytes)
        );
        assert_eq!(client.calls(), 3);
        assert!(!quota.load(Ordering::Relaxed));
        assert!(store.find_by_url(&pending.url).await.unwrap().is_none());
        assert!(store
            .find_session_projection_versions()
            .await
            .unwrap()
            .is_empty());
        let usage = std::fs::read_to_string(ledger).unwrap();
        assert_eq!(usage.lines().count(), 3);
        assert!(!usage.contains("synthetic-private-summary"));
        let (code, diagnostic) = content_rejection(&error).unwrap();
        assert_eq!(code, "facet_request_too_large");
        assert!(diagnostic.contains("已完成 3 个分块"));
        assert!(diagnostic.contains(&expected_bytes.to_string()));
        assert!(!diagnostic.contains("synthetic-private-summary"));
    }

    #[tokio::test]
    async fn facet_request_quarantine_keeps_batch_incomplete_but_allows_unrelated_session() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("quarantine.jsonl");
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let doc_store: Arc<dyn DocumentRepository> = store.clone();
        let client = Arc::new(RecordingClient::new(Vec::new(), None));
        let client_dyn: Arc<dyn LlmClient> = client.clone();
        let oversized = synthetic_session(vec![message("private-synthetic-marker".repeat(3_000))]);
        let normal = synthetic_session(vec![message("small synthetic input".to_string())]);
        let error = process_pending_sessions(
            vec![
                pending(&oversized, "remem://synthetic/large"),
                pending(&normal, "remem://synthetic/normal"),
            ],
            0,
            0,
            0,
            0,
            HashSet::new(),
            false,
            false,
            Some(QuarantineStore::load_from(path.clone()).unwrap()),
            doc_store.clone(),
            Some(client_dyn.clone()),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("摄入不完整"));
        assert_eq!(client.calls(), 1);
        assert!(store
            .find_by_url("remem://synthetic/normal")
            .await
            .unwrap()
            .is_some());
        assert!(store
            .find_by_url("remem://synthetic/large")
            .await
            .unwrap()
            .is_none());
        let record = std::fs::read_to_string(&path).unwrap();
        assert_eq!(record.lines().count(), 1);
        assert!(record.contains("facet_request_too_large"));
        assert!(!record.contains("private-synthetic-marker"));
        let skipped = process_pending_sessions(
            vec![pending(&oversized, "remem://synthetic/large")],
            0,
            0,
            0,
            0,
            HashSet::new(),
            false,
            false,
            Some(QuarantineStore::load_from(path).unwrap()),
            doc_store,
            Some(client_dyn),
        )
        .await;
        assert!(
            skipped.is_err(),
            "quarantined selection cannot report successful completion"
        );
        assert_eq!(
            client.calls(),
            1,
            "unchanged quarantined input is not automatically retried"
        );
    }
}

pub(super) fn log_preview(message: &str, max_chars: usize) -> String {
    let mut chars = message.chars();
    let mut preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}
