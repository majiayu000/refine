//! ingest-sessions 命令实现
//!
//! 默认优先从 remem raw archive 读取；remem 不存在时自动扫描本地会话
//! 文件；支持可配置并发、断点续传和 API 限流重试

use crate::cli::IngestProvider;
use crate::remem_sessions::{
    is_missing_remem_executable, load_remem_session, load_remem_session_summaries,
    RememSessionSummary,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::error::InfraError;
use refine_core::infra::{
    llm_with_retry_policy, LlmClient, LlmRetryPolicy, DEFAULT_MAX_RETRIES,
    DEFAULT_RETRY_BASE_DELAY_SECS,
};
use refine_core::knowledge::{Document, DocumentRepository, RestoreDocumentParams};
use refine_core::session::{
    build_facet_prompt, chunk_session, discover_sessions, facets_to_items, needs_chunking,
    parse_facet_response, parse_session_file, FilterConfig, SessionSource, FACET_SYSTEM_PROMPT,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Semaphore;

mod legacy_migration;
mod quarantine;

use quarantine::{record_key as quarantine_key, QuarantineStore};

const DEFAULT_CONCURRENCY: usize = 1;

fn concurrency() -> usize {
    std::env::var("REFINE_INGEST_CONCURRENCY")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(DEFAULT_CONCURRENCY)
}

pub struct IngestOptions {
    pub source: Option<SessionSource>,
    pub provider: IngestProvider,
    pub limit: Option<usize>,
    /// 按 mtime 降序取最近 N 个会话，与 limit 互斥
    pub latest: Option<usize>,
    pub dry_run: bool,
    pub retry_quarantined: bool,
}

#[derive(Debug)]
enum AutoProviderSelection<T> {
    Remem(T),
    LocalFallback,
}

fn select_auto_provider<T, F>(load_remem: F) -> Result<AutoProviderSelection<T>>
where
    F: FnOnce() -> Result<T>,
{
    match load_remem() {
        Ok(value) => Ok(AutoProviderSelection::Remem(value)),
        Err(error) if is_missing_remem_executable(&error) => {
            Ok(AutoProviderSelection::LocalFallback)
        }
        Err(error) => Err(error),
    }
}

/// 待处理的会话（已通过去重和过滤）
struct PendingSession {
    idx: usize,
    total: usize,
    url: String,
    source: SessionSource,
    project: Option<String>,
    captured_at: DateTime<Utc>,
    has_embedded_timestamp: bool,
    raw_content: String,
    source_version: Option<String>,
    needs_chunk: bool,
    chunks: Vec<String>,
    existing_document: Option<Document>,
    legacy_documents_to_delete: Vec<refine_core::knowledge::DocumentId>,
}

fn project_for_ingest(
    discovered_project: Option<&str>,
    session_project: Option<&str>,
) -> Option<String> {
    discovered_project
        .or(session_project)
        .map(ToOwned::to_owned)
}

fn session_captured_at(
    session_started_at: Option<DateTime<Utc>>,
    file_modified_at: SystemTime,
) -> DateTime<Utc> {
    session_started_at.unwrap_or_else(|| DateTime::<Utc>::from(file_modified_at))
}

fn session_needs_refresh(existing_doc: &Document, file_modified_at: SystemTime) -> bool {
    let file_modified_at = DateTime::<Utc>::from(file_modified_at);
    file_modified_at > existing_doc.updated_at()
}

fn content_source_version(provider: &str, raw_content: &str) -> String {
    let digest = Sha256::digest(raw_content.as_bytes());
    format!("{provider}:v2:sha256:{digest:x}")
}

fn document_with_source_version(document: &Document, source_version: &str) -> Document {
    Document::restore(RestoreDocumentParams {
        id: document.id().clone(),
        title: document.title().map(ToOwned::to_owned),
        raw_content: document.raw_content().to_string(),
        source: document.source().to_string(),
        url: document.url().to_string(),
        source_version: Some(source_version.to_string()),
        captured_at: document.captured_at(),
        created_at: document.created_at(),
        updated_at: document.updated_at(),
    })
}

fn incremental_cursor_path(home: &Path, source: Option<&SessionSource>, db_path: &Path) -> PathBuf {
    let source_key = match source {
        Some(SessionSource::ClaudeCode) => "claude-code",
        Some(SessionSource::Codex) => "codex",
        Some(SessionSource::RememRaw) => "remem-raw",
        None => "all",
    };
    let db_key = encode_path_for_filename(db_path);
    home.join(".refine")
        .join("ingest-cursors")
        .join(format!("last-ingest-mtime-{source_key}-{db_key}"))
}

fn encode_path_for_filename(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .to_string_lossy()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Read the Unix-second timestamp from the scoped ingest cursor file.
fn read_last_ingest_mtime(source: Option<&SessionSource>, db_path: &Path) -> Option<SystemTime> {
    let home = dirs::home_dir()?;
    let path = incremental_cursor_path(&home, source, db_path);
    let secs: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// Persist the Unix-second timestamp to the scoped ingest cursor file.
fn write_last_ingest_mtime(source: Option<&SessionSource>, db_path: &Path, t: SystemTime) {
    let Some(home) = dirs::home_dir() else { return };
    let path = incremental_cursor_path(&home, source, db_path);
    let Some(dir) = path.parent() else { return };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("failed to create {}: {}", dir.display(), e);
        return;
    }
    if let Ok(dur) = t.duration_since(SystemTime::UNIX_EPOCH) {
        if let Err(e) = std::fs::write(&path, dur.as_secs().to_string()) {
            tracing::warn!("failed to write {}: {}", path.display(), e);
        }
    }
}

pub async fn handle_ingest_sessions(
    options: IngestOptions,
    db_path: &Path,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    if options.source.is_some() && options.provider != IngestProvider::Local {
        anyhow::bail!(
            "--source requires --provider local because remem does not expose a trustworthy Claude/Codex source"
        );
    }

    match options.provider {
        IngestProvider::Local => {
            println!("provider=local");
            handle_legacy_ingest_sessions(options, db_path, doc_store, llm_client).await
        }
        IngestProvider::Remem => {
            println!("provider=remem");
            handle_remem_ingest_sessions(options, doc_store, llm_client).await
        }
        IngestProvider::Auto => {
            println!("provider=requested:auto");
            match select_auto_provider(|| {
                load_remem_session_summaries(options.limit, options.latest)
            }) {
                Ok(AutoProviderSelection::Remem(summaries)) => {
                    println!("provider=selected:remem");
                    handle_remem_ingest_sessions_with_summaries(
                        options, summaries, doc_store, llm_client,
                    )
                    .await
                }
                Ok(AutoProviderSelection::LocalFallback) => {
                    println!("provider=selected:local (auto fallback: remem executable not found)");
                    handle_legacy_ingest_sessions(options, db_path, doc_store, llm_client).await
                }
                Err(error) => Err(error.context("failed to load session summaries from remem")),
            }
        }
    }
}

async fn handle_remem_ingest_sessions(
    options: IngestOptions,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    let summaries = load_remem_session_summaries(options.limit, options.latest)
        .context("failed to load session summaries from remem")?;
    handle_remem_ingest_sessions_with_summaries(options, summaries, doc_store, llm_client).await
}

async fn handle_remem_ingest_sessions_with_summaries(
    options: IngestOptions,
    summaries: Vec<RememSessionSummary>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    if options.source.is_some() {
        anyhow::bail!(
            "--source requires --provider local because remem does not expose a trustworthy Claude/Codex source"
        );
    }

    println!("remem 返回 {} 个会话摘要", summaries.len());

    let total = summaries.len();
    let filter_config = FilterConfig::default();
    let document_count = doc_store.count().await?;
    let existing_documents = doc_store.find_recent(0, document_count).await?;
    let existing_remem_documents: HashMap<&str, &Document> = existing_documents
        .iter()
        .filter(|document| document.source() == "remem-raw-session")
        .map(|document| (document.url(), document))
        .collect();
    let mut claimed_legacy_documents = HashSet::new();
    let mut pending = Vec::new();
    let mut skipped_dup = 0usize;
    let mut skipped_filter = 0usize;
    let mut stale_refresh = 0usize;
    let mut fully_loaded = 0usize;

    for (idx, summary) in summaries.into_iter().enumerate() {
        let url = summary.stable_document_url();
        let existing_document = existing_remem_documents.get(url.as_str()).copied().cloned();
        if summary.user_message_count < filter_config.min_user_messages as i64 {
            skipped_filter += 1;
            continue;
        }

        let legacy_identity_is_unique = summary.legacy_identity_is_unique;
        let remem_session = load_remem_session(summary)
            .with_context(|| format!("failed to load full remem session for {url}"))?;
        fully_loaded += 1;
        let raw_content = remem_session.session.to_document_content();
        let source_version = content_source_version("remem", &raw_content);
        let legacy_documents_to_delete = if legacy_identity_is_unique {
            let document_ids = legacy_migration::matching_legacy_document_ids(
                &existing_documents,
                &remem_session,
                &raw_content,
            )?;
            for document_id in &document_ids {
                if !claimed_legacy_documents.insert(document_id.clone()) {
                    anyhow::bail!(
                        "legacy document {document_id} ambiguously matches multiple remem sessions"
                    );
                }
            }
            document_ids
        } else if let Some(document_id) =
            legacy_migration::legacy_document_covering_nonunique_summary(
                &existing_documents,
                &remem_session,
                &raw_content,
            )
        {
            if legacy_migration::claim_legacy_coverage_once(
                &mut claimed_legacy_documents,
                Some(document_id),
            ) {
                skipped_dup += 1;
                continue;
            }
            Vec::new()
        } else {
            Vec::new()
        };
        if let Some(existing_doc) = existing_document.as_ref() {
            if existing_doc.raw_content() == raw_content {
                if options.dry_run {
                    if existing_doc.source_version() != Some(source_version.as_str()) {
                        println!("  [dry-run] {url} | would backfill source snapshot metadata");
                    }
                    if !legacy_documents_to_delete.is_empty() {
                        println!(
                            "  [dry-run] {} | would remove {} superseded legacy document(s)",
                            url,
                            legacy_documents_to_delete.len()
                        );
                    }
                } else {
                    if existing_doc.source_version() != Some(source_version.as_str()) {
                        let versioned_document =
                            document_with_source_version(existing_doc, &source_version);
                        doc_store
                            .save(&versioned_document)
                            .await
                            .context("save remem source snapshot metadata")?;
                    }
                    doc_store
                        .delete_documents_with_items(&legacy_documents_to_delete)
                        .await
                        .context("delete superseded legacy documents and facets")?;
                }
                skipped_dup += 1;
                continue;
            }
            stale_refresh += 1;
        }

        if !refine_core::session::passes_filter(&remem_session.session, &filter_config) {
            skipped_filter += 1;
            continue;
        }

        if options.dry_run {
            println!(
                "  [dry-run] {} | {} msgs | {} chars | remem",
                url,
                remem_session.session.messages.len(),
                remem_session.session.char_count(),
            );
            continue;
        }

        let captured_at = DateTime::<Utc>::from_timestamp(remem_session.first_epoch, 0)
            .with_context(|| {
                format!(
                    "remem session {} has invalid first_epoch {}",
                    remem_session.session_id, remem_session.first_epoch
                )
            })?;
        let chunks = if needs_chunking(&remem_session.session) {
            chunk_session(&remem_session.session)
                .into_iter()
                .map(|chunk| chunk.content)
                .collect()
        } else {
            Vec::new()
        };

        pending.push(PendingSession {
            idx,
            total,
            url,
            source: remem_session.session.source,
            project: Some(remem_session.project),
            captured_at,
            has_embedded_timestamp: true,
            raw_content,
            source_version: Some(source_version),
            needs_chunk: !chunks.is_empty(),
            chunks,
            existing_document,
            legacy_documents_to_delete,
        });
    }

    println!("摘要预筛选后拉取了 {fully_loaded}/{total} 个完整会话");

    process_pending_sessions(
        pending,
        total,
        skipped_dup,
        skipped_filter,
        stale_refresh,
        options.dry_run,
        options.retry_quarantined,
        doc_store,
        llm_client,
    )
    .await
}

async fn handle_legacy_ingest_sessions(
    options: IngestOptions,
    db_path: &Path,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    // Incremental scan: only active for full (no --limit/--latest) non-dry-run runs.
    // 1-hour overlap on the cutoff absorbs clock skew and files modified near the boundary.
    let incremental = options.limit.is_none() && options.latest.is_none() && !options.dry_run;
    let source = options.source.clone();
    let scan_start = SystemTime::now();
    let mtime_after = if incremental {
        read_last_ingest_mtime(source.as_ref(), db_path).map(|last| {
            last.checked_sub(Duration::from_secs(3600))
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
    } else {
        None
    };

    let mut discovered = discover_sessions(source.clone(), mtime_after);
    if mtime_after.is_some() {
        println!("增量扫描：发现 {} 个会话文件", discovered.len());
    } else {
        println!("发现 {} 个会话文件", discovered.len());
    }

    // --latest: sort by mtime descending, keep N most recent
    if let Some(n) = options.latest {
        discovered.sort_by_key(|d| std::cmp::Reverse(d.modified_at));
        discovered.truncate(n);
    }

    // --limit: path-ordered take (only active when latest is None, enforced by clap)
    let sessions_to_process: Vec<_> = match options.limit {
        Some(limit) => discovered.into_iter().take(limit).collect(),
        None => discovered,
    };

    let total = sessions_to_process.len();
    let filter_config = FilterConfig::default();
    let document_count = doc_store.count().await?;
    let existing_documents = doc_store.find_recent(0, document_count).await?;
    let mut claimed_remem_documents = HashSet::new();
    let mut pending = Vec::new();
    let mut skipped_dup = 0usize;
    let mut skipped_filter = 0usize;
    let mut stale_refresh = 0usize;

    // 阶段 1: 串行做去重 + 过滤 + 解析（快，不需要 LLM）
    for (idx, ds) in sessions_to_process.iter().enumerate() {
        let legacy_url = ds.path.to_string_lossy().to_string();
        let legacy_document = doc_store.find_by_url(&legacy_url).await?;

        let session = match parse_session_file(&ds.path, ds.source.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("解析失败 {}: {}", legacy_url, e);
                continue;
            }
        };

        let project = project_for_ingest(ds.project.as_deref(), session.meta.project.as_deref());
        let has_embedded_timestamp = session.meta.started_at.is_some();
        let captured_at = session_captured_at(session.meta.started_at, ds.modified_at);
        let raw_content = session.to_document_content();
        let source_version = content_source_version("local", &raw_content);
        let remem_document = legacy_migration::matching_remem_document(
            &existing_documents,
            &ds.path,
            captured_at,
            &raw_content,
        )?;

        let mut legacy_documents_to_delete = Vec::new();
        let (url, effective_source, existing_document) = if let Some(remem_doc) = remem_document {
            legacy_migration::claim_remem_document_once(
                &mut claimed_remem_documents,
                &remem_doc,
                &ds.path,
            )?;
            if let Some(legacy_doc) = legacy_document.as_ref() {
                legacy_documents_to_delete.push(legacy_doc.id().clone());
            }
            if remem_doc.raw_content() == raw_content
                || remem_doc.raw_content().starts_with(&raw_content)
            {
                if options.dry_run {
                    if !legacy_documents_to_delete.is_empty() {
                        println!(
                            "  [dry-run] {} | would remove superseded legacy document",
                            legacy_url
                        );
                    }
                } else {
                    doc_store
                        .delete_documents_with_items(&legacy_documents_to_delete)
                        .await
                        .context("delete superseded legacy documents and facets")?;
                }
                skipped_dup += 1;
                continue;
            }
            // A local archive is never authoritative over a canonical remem
            // document. If it is not a prefix/equal snapshot, keep it under
            // its local identity instead of replacing remem data.
            if let Some(existing_doc) = legacy_document.as_ref() {
                if session_needs_refresh(existing_doc, ds.modified_at) {
                    stale_refresh += 1;
                } else {
                    skipped_dup += 1;
                    continue;
                }
            }
            (legacy_url, ds.source.clone(), legacy_document)
        } else {
            if let Some(existing_doc) = legacy_document.as_ref() {
                if session_needs_refresh(existing_doc, ds.modified_at) {
                    stale_refresh += 1;
                } else {
                    skipped_dup += 1;
                    continue;
                }
            }
            (legacy_url, ds.source.clone(), legacy_document)
        };

        if !refine_core::session::passes_filter(&session, &filter_config) {
            skipped_filter += 1;
            continue;
        }

        if options.dry_run {
            println!(
                "  [dry-run] {} | {} msgs | {} chars | {:?}",
                url,
                session.messages.len(),
                session.char_count(),
                effective_source,
            );
            continue;
        }
        let chunks = if needs_chunking(&session) {
            let cs = chunk_session(&session);
            cs.iter().map(|c| c.content.clone()).collect()
        } else {
            Vec::new()
        };

        pending.push(PendingSession {
            idx,
            total,
            url,
            source: effective_source,
            project,
            captured_at,
            has_embedded_timestamp,
            raw_content,
            source_version: Some(source_version),
            needs_chunk: !chunks.is_empty(),
            chunks,
            existing_document,
            legacy_documents_to_delete,
        });
    }

    process_pending_sessions(
        pending,
        total,
        skipped_dup,
        skipped_filter,
        stale_refresh,
        options.dry_run,
        options.retry_quarantined,
        doc_store,
        llm_client,
    )
    .await?;

    // Advance the incremental scan cursor so the next run only sees newer files.
    if incremental && !options.dry_run {
        write_last_ingest_mtime(source.as_ref(), db_path, scan_start);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_pending_sessions(
    mut pending: Vec<PendingSession>,
    total: usize,
    skipped_dup: usize,
    skipped_filter: usize,
    stale_refresh: usize,
    dry_run: bool,
    retry_quarantined: bool,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    if dry_run {
        let dry_count = total - skipped_dup - skipped_filter;
        println!(
            "\n[dry-run] 可处理 {}, 跳过重复 {}, 过滤 {}, 刷新过期 {}",
            dry_count, skipped_dup, skipped_filter, stale_refresh
        );
        return Ok(());
    }

    let mut quarantine = QuarantineStore::load()?;
    let selected_identities: HashSet<String> = pending
        .iter()
        .map(|session| quarantine_key(&session.url, session.source_version.as_deref()))
        .collect();
    let mut skipped_quarantined = 0usize;
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

    for ps in pending {
        let sem = semaphore.clone();
        let client = client.clone();
        let doc_store = doc_store.clone();
        let processed = processed.clone();
        let failed = failed.clone();
        let total_items = total_items.clone();
        let quota_hit = quota_hit.clone();
        let succeeded_sessions = succeeded_sessions.clone();
        let rejected_sessions = rejected_sessions.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            match process_single_session(&ps, &client, &doc_store, &quota_hit).await {
                Ok(item_count) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    total_items.fetch_add(item_count, Ordering::Relaxed);
                    succeeded_sessions
                        .lock()
                        .expect("succeeded URL lock poisoned")
                        .insert((ps.url.clone(), ps.source_version.clone()));
                }
                Err(error) => {
                    if let Some((code, message)) = content_rejection(&error) {
                        eprintln!(
                            "  ⛔ [{}/{}] 隔离: {} ({})",
                            ps.idx + 1,
                            ps.total,
                            code,
                            ps.url
                        );
                        rejected_sessions
                            .lock()
                            .expect("rejected session lock poisoned")
                            .push((ps.url.clone(), ps.source_version.clone(), code, message));
                    } else {
                        eprintln!("  ✗ [{}/{}] 失败: {}", ps.idx + 1, ps.total, error);
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
    for (url, _) in succeeded_sessions
        .lock()
        .expect("succeeded URL lock poisoned")
        .iter()
    {
        quarantine.resolve(url);
    }
    let rejected = rejected_sessions
        .lock()
        .expect("rejected session lock poisoned");
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

async fn process_single_session(
    ps: &PendingSession,
    client: &Arc<dyn LlmClient>,
    doc_store: &Arc<dyn DocumentRepository>,
    quota_hit: &Arc<AtomicBool>,
) -> Result<usize> {
    let content = if ps.needs_chunk {
        let total_chunks = ps.chunks.len();
        let mut summaries = Vec::with_capacity(total_chunks);
        for (idx, chunk) in ps.chunks.iter().enumerate() {
            let text = llm_call_with_retry(client, chunk, quota_hit)
                .await
                .with_context(|| {
                    format!(
                        "分块 {}/{} 提取失败，整个 session 视为失败以避免数据缺失",
                        idx + 1,
                        total_chunks
                    )
                })?;
            summaries.push(text);
        }
        summaries.join("\n\n---\n\n")
    } else {
        ps.raw_content.clone()
    };

    let facet_response = extract_and_parse_facets_with_retry(&content, client, quota_hit).await?;

    let doc = build_session_document(ps, &facet_response.session_summary);
    let items = facets_to_items(&facet_response, doc.id(), ps.project.as_deref());
    let item_count = items.len();
    doc_store
        .save_with_replaced_items_and_delete_documents(&doc, &items, &ps.legacy_documents_to_delete)
        .await
        .context("保存 Document/Items 并清理旧会话失败")?;

    println!(
        "  + [{}/{}] {} | {} items",
        ps.idx + 1,
        ps.total,
        facet_response.session_summary,
        item_count,
    );

    Ok(item_count)
}

fn content_rejection(error: &anyhow::Error) -> Option<(String, String)> {
    error.chain().find_map(|cause| {
        cause
            .downcast_ref::<InfraError>()
            .and_then(|infra_error| match infra_error {
                InfraError::LlmRejected { code, message } => Some((code.clone(), message.clone())),
                _ => None,
            })
    })
}

fn build_session_document(ps: &PendingSession, title: &str) -> Document {
    if let Some(existing_doc) = &ps.existing_document {
        let captured_at = if ps.has_embedded_timestamp {
            ps.captured_at
        } else {
            existing_doc.captured_at()
        };

        return Document::restore(RestoreDocumentParams {
            id: existing_doc.id().clone(),
            title: Some(title.to_string()),
            raw_content: ps.raw_content.clone(),
            source: ps.source.as_str().to_string(),
            url: ps.url.clone(),
            source_version: ps.source_version.clone(),
            captured_at,
            created_at: existing_doc.created_at(),
            updated_at: Utc::now(),
        });
    }

    let mut doc = Document::new(ps.source.as_str(), &ps.raw_content);
    doc.set_title(title);
    doc.set_url(&ps.url);
    doc.set_source_version(ps.source_version.as_deref());
    doc.set_captured_at(ps.captured_at);
    doc
}

async fn llm_call_with_retry(
    client: &Arc<dyn LlmClient>,
    content: &str,
    quota_hit: &Arc<AtomicBool>,
) -> Result<String> {
    if quota_hit.load(Ordering::Relaxed) {
        return Err(anyhow::anyhow!("LLM 配额已耗尽，跳过"));
    }

    let prompt = build_facet_prompt(content);
    let result = llm_with_retry_policy(
        client,
        &prompt,
        FACET_SYSTEM_PROMPT,
        LlmRetryPolicy::default(),
        |attempt, max_retries, delay_secs, err| {
            let message = err.to_string();
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

fn finish_llm_call(
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
        Err(err) => Err(anyhow::Error::new(err)),
    }
}

async fn extract_and_parse_facets_with_retry(
    content: &str,
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
) -> Result<refine_core::session::FacetResponse> {
    extract_and_parse_facets_with_retry_policy(
        content,
        client,
        quota_hit,
        DEFAULT_MAX_RETRIES,
        DEFAULT_RETRY_BASE_DELAY_SECS,
    )
    .await
}

async fn extract_and_parse_facets_with_retry_policy(
    content: &str,
    client: &Arc<dyn LlmClient>,
    quota_hit: &Arc<AtomicBool>,
    max_retries: usize,
    base_delay_secs: u64,
) -> Result<refine_core::session::FacetResponse> {
    let max_retries = max_retries.max(1);

    for attempt in 0..max_retries {
        let response = llm_call_with_retry(client, content, quota_hit).await?;
        match parse_facet_response(&response) {
            Ok(facets) => return Ok(facets),
            Err(err) if attempt == max_retries - 1 => return Err(anyhow::anyhow!(err)),
            Err(err) => {
                let delay_secs = ingest_retry_delay_secs(base_delay_secs, attempt);
                eprintln!(
                    "    ⏳ 解析重试 ({}/{}) 等待 {}s: {}",
                    attempt + 1,
                    max_retries,
                    delay_secs,
                    log_preview(&err, 80),
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

fn log_preview(message: &str, max_chars: usize) -> String {
    let mut chars = message.chars();
    let mut preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
#[path = "ingest_sessions/tests.rs"]
mod tests;
