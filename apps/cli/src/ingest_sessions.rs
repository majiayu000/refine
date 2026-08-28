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
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentRepository, RestoreDocumentParams};
use refine_core::session::{
    chunk_session, discover_sessions, needs_chunking, parse_session_file, FilterConfig,
    SessionMode, SessionSource,
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod cursor;
mod legacy_migration;
mod provenance;
mod quarantine;
mod worker;

use cursor::{
    cursor_failure, lock_incremental_cursor, lock_session_mutations, read_last_ingest_mtime,
    safe_cursor_watermark, write_last_ingest_mtime, CursorPurpose,
};

pub(crate) fn lock_session_mutations_for_repair(db_path: &Path) -> Result<std::fs::File> {
    cursor::try_lock_session_mutations(db_path)
}
use provenance::backfill_session_metadata;
use worker::{process_pending_sessions, PendingSession};

#[cfg(test)]
use cursor::{
    incremental_cursor_path, parse_ingest_cursor, unix_seconds, write_ingest_cursor_at,
    IngestCursorFailure, IngestCursorState, INGEST_CURSOR_VERSION,
};
#[cfg(test)]
use worker::{
    content_rejection, extract_and_parse_facets_with_retry_policy, finish_llm_call,
    llm_call_with_retry, log_preview, process_single_session,
};

pub struct IngestOptions {
    pub source: Option<SessionSource>,
    pub provider: IngestProvider,
    pub limit: Option<usize>,
    /// 按 mtime 降序取最近 N 个会话，与 limit 互斥
    pub latest: Option<usize>,
    pub dry_run: bool,
    pub retry_quarantined: bool,
    pub backfill_session_metadata: bool,
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

fn project_for_ingest(
    discovered_project: Option<&str>,
    session_project: Option<&str>,
) -> Option<String> {
    discovered_project
        .or(session_project)
        .map(ToOwned::to_owned)
}

fn project_identity_for_ingest(
    selected_project: Option<&str>,
    session_project_identity: Option<&str>,
) -> Option<String> {
    session_project_identity
        .or(selected_project)
        .map(ToOwned::to_owned)
}

fn session_captured_at(
    session_started_at: Option<DateTime<Utc>>,
    file_modified_at: SystemTime,
) -> DateTime<Utc> {
    session_started_at.unwrap_or_else(|| DateTime::<Utc>::from(file_modified_at))
}

fn session_needs_refresh(existing_doc: &Document, source_version: &str, raw_content: &str) -> bool {
    match existing_doc.source_version() {
        Some(existing_version) => existing_version != source_version,
        None => existing_doc.raw_content() != raw_content,
    }
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
    if options.backfill_session_metadata
        && (options.provider != IngestProvider::Local
            || options.source.as_ref() != Some(&SessionSource::Codex))
    {
        anyhow::bail!("--backfill-session-metadata requires --provider local --source codex");
    }

    match options.provider {
        IngestProvider::Local => {
            println!("provider=local");
            handle_legacy_ingest_sessions(options, db_path, doc_store, llm_client).await
        }
        IngestProvider::Remem => {
            println!("provider=remem");
            handle_remem_ingest_sessions(options, db_path, doc_store, llm_client).await
        }
        IngestProvider::Auto => {
            println!("provider=requested:auto");
            match select_auto_provider(|| {
                load_remem_session_summaries(options.limit, options.latest)
            }) {
                Ok(AutoProviderSelection::Remem(summaries)) => {
                    println!("provider=selected:remem");
                    handle_remem_ingest_sessions_with_summaries(
                        options, db_path, summaries, doc_store, llm_client,
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
    db_path: &Path,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    let summaries = load_remem_session_summaries(options.limit, options.latest)
        .context("failed to load session summaries from remem")?;
    handle_remem_ingest_sessions_with_summaries(options, db_path, summaries, doc_store, llm_client)
        .await
}

async fn handle_remem_ingest_sessions_with_summaries(
    options: IngestOptions,
    db_path: &Path,
    summaries: Vec<RememSessionSummary>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    if options.source.is_some() {
        anyhow::bail!(
            "--source requires --provider local because remem does not expose a trustworthy Claude/Codex source"
        );
    }
    let _mutation_lock = if options.dry_run {
        None
    } else {
        Some(lock_session_mutations(db_path)?)
    };

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
            project_identity: Some(remem_session.project.clone()),
            project: Some(remem_session.project),
            mode: SessionMode::Unknown,
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
    let cursor_purpose = if options.backfill_session_metadata {
        CursorPurpose::Metadata
    } else {
        CursorPurpose::Ingest
    };
    // Serialize the complete read/scan/process/write cycle so an older,
    // slower run cannot overwrite a newer safe watermark or failure set.
    let _cursor_lock = if incremental {
        Some(lock_incremental_cursor(
            source.as_ref(),
            db_path,
            cursor_purpose,
        )?)
    } else {
        None
    };
    let _mutation_lock = if options.dry_run {
        None
    } else {
        Some(lock_session_mutations(db_path)?)
    };
    let scan_start = SystemTime::now();
    let mtime_after = if incremental {
        read_last_ingest_mtime(source.as_ref(), db_path, cursor_purpose).map(|last| {
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
    let mut parse_failures = Vec::new();
    let mut metadata_backfilled = 0usize;
    let mut metadata_already_current = 0usize;
    let mut metadata_missing = 0usize;

    // 阶段 1: 串行做去重 + 过滤 + 解析（快，不需要 LLM）
    for (idx, ds) in sessions_to_process.iter().enumerate() {
        let legacy_url = ds.path.to_string_lossy().to_string();
        let legacy_document = doc_store.find_by_url(&legacy_url).await?;

        let session = match parse_session_file(&ds.path, ds.source.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("解析失败 {}: {}", legacy_url, e);
                parse_failures.push(cursor_failure(&ds.path, ds.modified_at, "parse_error"));
                continue;
            }
        };
        if session.meta.truncated_tail {
            tracing::warn!(
                path = %ds.path.display(),
                "会话文件尾部仍在写入；本次不保存，下一轮重试"
            );
            parse_failures.push(cursor_failure(&ds.path, ds.modified_at, "truncated_tail"));
            continue;
        }

        let project = project_for_ingest(ds.project.as_deref(), session.meta.project.as_deref());
        let project_identity = project_identity_for_ingest(
            project.as_deref(),
            session.meta.project_identity.as_deref(),
        );
        let mode = session.meta.mode;
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

        if options.backfill_session_metadata {
            if !refine_core::session::passes_filter(&session, &filter_config) {
                skipped_filter += 1;
                continue;
            }
            let existing = remem_document.as_ref().or(legacy_document.as_ref());
            match existing {
                Some(document) if options.dry_run => {
                    if backfill_session_metadata(&doc_store, document, mode, false).await? {
                        println!("  [dry-run] {} | would backfill {:?}", legacy_url, mode);
                        metadata_backfilled += 1;
                    } else {
                        metadata_already_current += 1;
                    }
                }
                Some(document) => {
                    if backfill_session_metadata(&doc_store, document, mode, true).await? {
                        metadata_backfilled += 1;
                    } else {
                        metadata_already_current += 1;
                    }
                }
                None => {
                    metadata_missing += 1;
                    parse_failures.push(cursor_failure(
                        &ds.path,
                        ds.modified_at,
                        "missing_document",
                    ));
                }
            }
            continue;
        }

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
                    backfill_session_metadata(&doc_store, &remem_doc, mode, true).await?;
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
                if session_needs_refresh(existing_doc, &source_version, &raw_content) {
                    stale_refresh += 1;
                } else {
                    if !options.dry_run {
                        backfill_session_metadata(&doc_store, existing_doc, mode, true).await?;
                    }
                    skipped_dup += 1;
                    continue;
                }
            }
            (legacy_url, ds.source.clone(), legacy_document)
        } else {
            if let Some(existing_doc) = legacy_document.as_ref() {
                if session_needs_refresh(existing_doc, &source_version, &raw_content) {
                    stale_refresh += 1;
                } else {
                    if !options.dry_run {
                        backfill_session_metadata(&doc_store, existing_doc, mode, true).await?;
                    }
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
            project_identity,
            mode,
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

    if options.backfill_session_metadata {
        if incremental {
            let failed_mtimes = parse_failures
                .iter()
                .map(|failure| UNIX_EPOCH + Duration::from_secs(failure.modified_at_secs))
                .collect::<Vec<_>>();
            let watermark = safe_cursor_watermark(scan_start, &failed_mtimes);
            write_last_ingest_mtime(
                source.as_ref(),
                db_path,
                cursor_purpose,
                watermark,
                parse_failures.clone(),
            )?;
        }
        println!(
            "会话来源元数据回填完成: 更新 {}, 已是最新 {}, 未找到既有文档 {}, 过滤 {}（未调用 LLM）",
            metadata_backfilled, metadata_already_current, metadata_missing, skipped_filter
        );
        if !parse_failures.is_empty() {
            anyhow::bail!(
                "{} 个会话元数据回填失败，cursor 已停在最早失败项之前",
                parse_failures.len()
            );
        }
        return Ok(());
    }

    let process_result = process_pending_sessions(
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
    .await;

    if incremental && process_result.is_ok() {
        let failed_mtimes = parse_failures
            .iter()
            .map(|failure| UNIX_EPOCH + Duration::from_secs(failure.modified_at_secs))
            .collect::<Vec<_>>();
        let watermark = safe_cursor_watermark(scan_start, &failed_mtimes);
        write_last_ingest_mtime(
            source.as_ref(),
            db_path,
            cursor_purpose,
            watermark,
            parse_failures.clone(),
        )?;
    }

    process_result?;
    if !parse_failures.is_empty() {
        anyhow::bail!("{} 个会话文件解析失败或仍在写入", parse_failures.len());
    }

    Ok(())
}

#[cfg(test)]
#[path = "ingest_sessions/tests.rs"]
mod tests;
