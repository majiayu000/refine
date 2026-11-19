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
    build_facet_prompt, facet_request_bytes, facets_to_items_with_mode_and_identity,
    parse_facet_response, prune_unknown_evidence_ids, session_projection_evidence,
    validate_facet_evidence, FacetResponse, SessionChunk, SessionMode, SessionSource,
    SourceMessageReference, FACET_REQUEST_MAX_BYTES, FACET_SYSTEM_PROMPT,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

const DEFAULT_CONCURRENCY: usize = 1;
const DEFAULT_FACET_PARSE_ATTEMPTS: usize = 2;

fn checked_facet_prompt(content: &str, stage: &'static str) -> Result<String, InfraError> {
    let prompt = build_facet_prompt(content);
    let request_bytes = facet_request_bytes(content);
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
    pub(super) chunks: Vec<SessionChunk>,
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
            // Same prefix as a partly failed batch below; the scheduled scripts match it.
            anyhow::bail!(
                "摄入不完整: 瞬态失败 0, 本次相关隔离 {selected_quarantine_count}（没有待处理会话）；队列: {}；确认上游策略已修复后使用 --retry-quarantined",
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
    // Explicitly retried sessions that now fail transiently were not rejected
    // deterministically; dropping their old record lets the next routine run
    // resume them instead of skipping them as quarantined forever.
    let transient_sessions = Arc::new(Mutex::new(HashSet::<String>::new()));
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
        let transient_sessions = transient_sessions.clone();
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
                        // `{:#}` keeps the cause chain; the outer context alone hides why it failed.
                        eprintln!(
                            "  ✗ [{}/{}] 失败: {:#}",
                            session.idx + 1,
                            session.total,
                            error
                        );
                        failed.fetch_add(1, Ordering::Relaxed);
                        if let Ok(mut transient) = transient_sessions.lock() {
                            transient.insert(session.url.clone());
                        }
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
    let transient = transient_sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("transient session lock poisoned"))?;
    for url in transient.iter() {
        quarantine.resolve(url);
    }
    drop(transient);
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
    let (facet_response, reduction_depth) = if session.needs_chunk {
        let total_chunks = session.chunks.len();
        // Reject an indivisible oversized message before spending on any chunk.
        // The common call boundary checks the same contract again for reduction
        // and regeneration; no source text or identity is changed.
        for chunk in &session.chunks {
            checked_facet_prompt(&chunk.content, "chunk extraction")?;
        }
        // Each chunk and the final reduction need at least one attempt.
        if let Some(budget) = client.run_budget() {
            let minimum = total_chunks.saturating_add(1) as u64;
            let limit = budget.max_attempts();
            if minimum > limit {
                anyhow::bail!("session needs at least {minimum} provider attempts, above this run's {limit}; raise REFINE_LLM_MAX_ATTEMPTS to retry it");
            }
        }
        let mut chunk_facets = Vec::with_capacity(total_chunks);
        for (idx, chunk) in session.chunks.iter().enumerate() {
            let facets = extract_and_parse_facets_with_retry(
                &chunk.content,
                client,
                quota_hit,
                &chunk.source_messages,
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
            chunk_facets.push(facets);
        }
        reduce_chunk_facets(chunk_facets, &session.source_messages, client, quota_hit)
            .await
            .map_err(|error| {
                if matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { .. })) {
                    error.context(format!(
                        "最终汇总失败；已完成 {} 个分块提炼（实际 provider attempts 保留在用量账本中），未发布会话",
                        total_chunks
                    ))
                } else {
                    error
                }
            })?
    } else {
        let content = session
            .facet_content
            .as_ref()
            .unwrap_or(&session.raw_content);
        let facets = extract_and_parse_facets_with_retry(
            content,
            client,
            quota_hit,
            &session.source_messages,
            "session extraction",
        )
        .await?;
        (facets, 0)
    };
    let document = build_session_document(session, &facet_response.session_summary);
    let mut items = facets_to_items_with_mode_and_identity(
        &facet_response,
        document.id(),
        session.project.as_deref(),
        session.project_identity.as_deref(),
        session.mode,
    );
    tag_reduction_depth(&mut items, reduction_depth)?;
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

/// Levels of LLM reduction above chunk extraction. Two levels cover ~10k chunks
/// at the observed 4–8KB per partial; a level that stops shrinking fails closed.
const MAX_REDUCTION_DEPTH: usize = 4;
const REDUCTION_SEPARATOR: &str = "\n\n---\n\n";

fn join_partials(parts: &[FacetResponse]) -> Result<String> {
    let rendered = parts
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rendered.join(REDUCTION_SEPARATOR))
}

/// Partials may cite only messages their own inputs already cited, so every
/// reduced observation still resolves to a validated original message.
fn cited_sources(
    all: &[SourceMessageReference],
    parts: &[FacetResponse],
) -> Vec<SourceMessageReference> {
    let cited: HashSet<_> = parts
        .iter()
        .flat_map(|part| part.evidence.iter())
        .flat_map(|entry| entry.message_ids.iter().copied())
        .collect();
    all.iter()
        .filter(|message| cited.contains(&message.id))
        .cloned()
        .collect()
}

/// Greedy, order-preserving packing by the rendered request size.
fn pack_partials(parts: &[FacetResponse]) -> Result<Vec<std::ops::Range<usize>>> {
    let mut groups = Vec::new();
    let mut start = 0;
    while start < parts.len() {
        let mut end = start + 1;
        while end < parts.len()
            && checked_facet_prompt(&join_partials(&parts[start..=end])?, "group reduction").is_ok()
        {
            end += 1;
        }
        groups.push(start..end);
        start = end;
    }
    Ok(groups)
}

/// Returns the session facets and the number of reduction levels used
/// (1 = the original single reduction; requests are byte-identical then).
async fn reduce_chunk_facets(
    chunk_facets: Vec<FacetResponse>,
    all_sources: &[SourceMessageReference],
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
) -> Result<(FacetResponse, usize)> {
    let mut level = chunk_facets;
    let mut depth = 1;
    loop {
        let content = join_partials(&level)?;
        let fits = checked_facet_prompt(&content, "final reduction");
        let groups = match fits {
            Ok(_) => None,
            Err(_) if depth >= MAX_REDUCTION_DEPTH => None,
            Err(_) => Some(pack_partials(&level)?).filter(|groups| groups.len() < level.len()),
        };
        let Some(groups) = groups else {
            // Fits, or cannot shrink further: the boundary check reports the
            // oversized final request deterministically (quarantine, no publish).
            let sources = cited_sources(all_sources, &level);
            let facets = extract_and_parse_facets_with_retry(
                &content,
                client,
                quota_hit,
                &sources,
                "final reduction",
            )
            .await?;
            return Ok((facets, depth));
        };
        let mut next = Vec::with_capacity(groups.len());
        for group in groups {
            let parts = &level[group];
            let sources = cited_sources(all_sources, parts);
            next.push(
                extract_and_parse_facets_with_retry(
                    &join_partials(parts)?,
                    client,
                    quota_hit,
                    &sources,
                    "group reduction",
                )
                .await?,
            );
        }
        level = next;
        depth += 1;
    }
}

fn tag_reduction_depth(items: &mut [refine_core::knowledge::Item], depth: usize) -> Result<()> {
    if depth < 2 {
        return Ok(());
    }
    let tag = refine_core::knowledge::Tag::new(&format!("facet_reduction_depth_{depth}"))
        .context("build reduction depth tag")?;
    for item in items {
        item.add_tag(tag.clone()).context("tag reduction depth")?;
    }
    Ok(())
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
        return Err(anyhow::anyhow!("本次运行的 LLM 预算（REFINE_LLM_MAX_ATTEMPTS / REFINE_LLM_MAX_TOKENS）或提供方配额已用完，留到下次运行"));
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
        match parse_facet_response(&response).and_then(|mut facets| {
            let removed = prune_unknown_evidence_ids(&mut facets, source_messages);
            if removed > 0 {
                eprintln!("    ⚠ {stage}: 去掉 {removed} 个不在本次原文中的证据消息 ID");
            }
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
mod request_limit_tests;

pub(super) fn log_preview(message: &str, max_chars: usize) -> String {
    let mut chars = message.chars();
    let mut preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}
