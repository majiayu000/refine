//! ingest-sessions 命令实现
//!
//! 支持 3 路并发 + 断点续传 + API 限流重试

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::error::InfraError;
use refine_core::infra::{llm_with_retry_policy, LlmClient, LlmRetryPolicy};
use refine_core::knowledge::{Document, DocumentRepository, RestoreDocumentParams};
use refine_core::session::{
    build_facet_prompt, chunk_session, discover_sessions, facets_to_items, needs_chunking,
    parse_facet_response, parse_session_file, FilterConfig, SessionSource, FACET_SYSTEM_PROMPT,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Semaphore;

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
    pub limit: Option<usize>,
    /// 按 mtime 降序取最近 N 个会话，与 limit 互斥
    pub latest: Option<usize>,
    pub dry_run: bool,
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
    needs_chunk: bool,
    chunks: Vec<String>,
    existing_document: Option<Document>,
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

fn incremental_cursor_path(home: &Path, source: Option<&SessionSource>, db_path: &Path) -> PathBuf {
    let source_key = match source {
        Some(SessionSource::ClaudeCode) => "claude-code",
        Some(SessionSource::Codex) => "codex",
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
    let mut pending = Vec::new();
    let mut skipped_dup = 0usize;
    let mut skipped_filter = 0usize;
    let mut stale_refresh = 0usize;

    // 阶段 1: 串行做去重 + 过滤 + 解析（快，不需要 LLM）
    for (idx, ds) in sessions_to_process.iter().enumerate() {
        let url = ds.path.to_string_lossy().to_string();

        let existing_document = doc_store.find_by_url(&url).await?;
        if let Some(existing_doc) = existing_document.as_ref() {
            if session_needs_refresh(existing_doc, ds.modified_at) {
                stale_refresh += 1;
            } else {
                skipped_dup += 1;
                continue;
            }
        }

        let session = match parse_session_file(&ds.path, ds.source.clone()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("解析失败 {}: {}", url, e);
                continue;
            }
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
                ds.source,
            );
            continue;
        }

        let project = project_for_ingest(ds.project.as_deref(), session.meta.project.as_deref());
        let has_embedded_timestamp = session.meta.started_at.is_some();
        let captured_at = session_captured_at(session.meta.started_at, ds.modified_at);

        let raw_content = session.to_document_content();
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
            source: ds.source.clone(),
            project,
            captured_at,
            has_embedded_timestamp,
            raw_content,
            needs_chunk: !chunks.is_empty(),
            chunks,
            existing_document,
        });
    }

    if options.dry_run {
        let dry_count = total - skipped_dup - skipped_filter;
        println!(
            "\n[dry-run] 可处理 {}, 跳过重复 {}, 过滤 {}, 刷新过期 {}",
            dry_count, skipped_dup, skipped_filter, stale_refresh
        );
        return Ok(());
    }

    let concurrency = concurrency();
    println!(
        "待处理 {} 个会话（跳过重复 {}, 过滤 {}, 刷新过期 {}），{} 路并发...\n",
        pending.len(),
        skipped_dup,
        skipped_filter,
        stale_refresh,
        concurrency,
    );

    if pending.is_empty() {
        println!("全部已处理完毕。");
        return Ok(());
    }

    // 阶段 2: 并发做 LLM 提取 + 保存
    let client = llm_client.ok_or_else(|| anyhow::anyhow!("非 dry-run 模式需要 LLM API Key"))?;
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let processed = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total_items = Arc::new(AtomicUsize::new(0));
    // Shared flag: when any worker hits RateLimited, all others abort pending retries.
    let quota_hit = Arc::new(AtomicBool::new(false));

    let mut handles = Vec::new();

    for ps in pending {
        let sem = semaphore.clone();
        let client = client.clone();
        let doc_store = doc_store.clone();
        let processed = processed.clone();
        let failed = failed.clone();
        let total_items = total_items.clone();
        let quota_hit = quota_hit.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");

            let result = process_single_session(&ps, &client, &doc_store, &quota_hit).await;

            match result {
                Ok(item_count) => {
                    processed.fetch_add(1, Ordering::Relaxed);
                    total_items.fetch_add(item_count, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("  ✗ [{}/{}] 失败: {}", ps.idx + 1, ps.total, e);
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.await;
    }

    let processed = processed.load(Ordering::Relaxed);
    let failed = failed.load(Ordering::Relaxed);
    let total_items = total_items.load(Ordering::Relaxed);

    println!(
        "\n完成: 处理 {}, 跳过重复 {}, 过滤 {}, 刷新过期 {}, 失败 {}, 生成 {} 条观测",
        processed, skipped_dup, skipped_filter, stale_refresh, failed, total_items
    );
    if failed > 0 {
        eprintln!("提示: 重新运行即可续传失败的会话");
        return Err(anyhow::anyhow!("{} 个会话提取失败", failed));
    }

    // Advance the incremental scan cursor so the next run only sees newer files.
    if incremental {
        write_last_ingest_mtime(source.as_ref(), db_path, scan_start);
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
            match llm_call_with_retry(client, chunk, quota_hit).await {
                Ok(text) => summaries.push(text),
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "分块 {}/{} 提取失败，整个 session 视为失败以避免数据缺失: {}",
                        idx + 1,
                        total_chunks,
                        e
                    ));
                }
            }
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
        .save_with_replaced_items(&doc, &items)
        .await
        .context("保存 Document/Items 失败")?;

    println!(
        "  + [{}/{}] {} | {} items",
        ps.idx + 1,
        ps.total,
        &facet_response.session_summary,
        item_count,
    );

    Ok(item_count)
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
            captured_at,
            created_at: existing_doc.created_at(),
            updated_at: Utc::now(),
        });
    }

    let mut doc = Document::new(ps.source.as_str(), &ps.raw_content);
    doc.set_title(title);
    doc.set_url(&ps.url);
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
    match llm_with_retry_policy(
        client,
        &prompt,
        FACET_SYSTEM_PROMPT,
        LlmRetryPolicy::default(),
        |attempt, max_retries, delay_secs, err| {
            let message = err.to_string();
            eprintln!(
                "    ⏳ 重试 ({}/{}) 等待 {}s: {}",
                attempt,
                max_retries,
                delay_secs,
                &message[..message.len().min(80)],
            );
        },
    )
    .await
    {
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
    let response = llm_call_with_retry(client, content, quota_hit).await?;
    parse_facet_response(&response).map_err(|e| anyhow::anyhow!(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use refine_core::error::InfraResult;
    use refine_core::infra::SqliteStore;
    use refine_core::knowledge::{DocumentRepository, Item, ItemRepository};
    use refine_core::session::discover_sessions_in;
    use std::fs;

    struct StaticLlmClient {
        response: String,
    }

    #[async_trait]
    impl LlmClient for StaticLlmClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn project_for_ingest_prefers_discovered_project_then_session_metadata() {
        assert_eq!(
            project_for_ingest(Some("claude-project"), Some("codex-cwd")).as_deref(),
            Some("claude-project")
        );
        assert_eq!(
            project_for_ingest(None, Some("codex-cwd")).as_deref(),
            Some("codex-cwd")
        );
        assert_eq!(project_for_ingest(None, None), None);
    }

    #[test]
    fn session_captured_at_prefers_session_started_at_then_file_mtime() {
        let started_at = Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap();
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        assert_eq!(session_captured_at(Some(started_at), mtime), started_at);

        let fallback = session_captured_at(None, mtime);
        assert_eq!(fallback, DateTime::<Utc>::from(mtime));
    }

    #[test]
    fn session_needs_refresh_when_file_mtime_is_newer_than_saved_document() {
        let mut doc = Document::new("codex-session", "raw");
        doc.set_url("file:///tmp/session.jsonl");

        let old_mtime = SystemTime::now()
            .checked_sub(Duration::from_secs(60))
            .unwrap();
        assert!(!session_needs_refresh(&doc, old_mtime));

        let new_mtime = SystemTime::now() + Duration::from_millis(200);
        assert!(session_needs_refresh(&doc, new_mtime));
    }

    #[test]
    fn scoped_cursor_keeps_other_sources_discoverable() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let db_path = tmp.path().join("refine.db");
        fs::write(&db_path, "").unwrap();

        let claude_dir = home.join(".claude/projects/proj");
        fs::create_dir_all(&claude_dir).unwrap();
        let claude_path = claude_dir.join("claude.jsonl");
        fs::write(&claude_path, "{}").unwrap();
        filetime::set_file_mtime(&claude_path, filetime::FileTime::from_unix_time(20_000, 0))
            .unwrap();

        let codex_dir = home.join(".codex/sessions");
        fs::create_dir_all(&codex_dir).unwrap();
        let codex_path = codex_dir.join("codex.jsonl");
        fs::write(&codex_path, "{}").unwrap();
        filetime::set_file_mtime(&codex_path, filetime::FileTime::from_unix_time(1_000, 0))
            .unwrap();

        write_last_ingest_mtime_at(
            home,
            Some(&SessionSource::ClaudeCode),
            &db_path,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10_000),
        );

        let claude_cutoff =
            read_last_ingest_mtime_at(home, Some(&SessionSource::ClaudeCode), &db_path)
                .unwrap()
                .checked_sub(Duration::from_secs(3600))
                .unwrap();
        let claude_discovered =
            discover_sessions_in(home, Some(SessionSource::ClaudeCode), Some(claude_cutoff));
        assert_eq!(claude_discovered.len(), 1);

        let codex_cutoff = read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_path)
            .map(|last| last.checked_sub(Duration::from_secs(3600)).unwrap());
        assert!(codex_cutoff.is_none());

        let codex_discovered = discover_sessions_in(home, Some(SessionSource::Codex), codex_cutoff);
        assert_eq!(codex_discovered.len(), 1);
        assert_eq!(codex_discovered[0].path, codex_path);
    }

    #[test]
    fn cursor_is_partitioned_by_database_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let db_a = tmp.path().join("a.db");
        let db_b = tmp.path().join("b.db");
        fs::write(&db_a, "").unwrap();
        fs::write(&db_b, "").unwrap();

        let when = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        write_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_a, when);

        assert_eq!(
            read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_a),
            Some(when)
        );
        assert_eq!(
            read_last_ingest_mtime_at(home, Some(&SessionSource::Codex), &db_b),
            None
        );
        assert_ne!(
            incremental_cursor_path(home, Some(&SessionSource::Codex), &db_a),
            incremental_cursor_path(home, Some(&SessionSource::Codex), &db_b)
        );
    }

    fn read_last_ingest_mtime_at(
        home: &Path,
        source: Option<&SessionSource>,
        db_path: &Path,
    ) -> Option<SystemTime> {
        let path = incremental_cursor_path(home, source, db_path);
        let secs: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn write_last_ingest_mtime_at(
        home: &Path,
        source: Option<&SessionSource>,
        db_path: &Path,
        t: SystemTime,
    ) {
        let path = incremental_cursor_path(home, source, db_path);
        let dir = path.parent().unwrap();
        fs::create_dir_all(dir).unwrap();
        let dur = t.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        fs::write(path, dur.as_secs().to_string()).unwrap();
    }

    #[tokio::test]
    async fn process_single_session_links_items_to_saved_document_id() {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
        let doc_store: Arc<dyn DocumentRepository> = store.clone();
        let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
            response: r#"{
                "session_summary": "修复文档关联",
                "cognitive_level": "proficient",
                "collaboration_mode": "pair_programming",
                "decisions": ["使用保存后的文档 ID"],
                "bugs_fixed": ["修复会话导入的 shadow document id"],
                "patterns": [],
                "friction": [],
                "project_progress": [],
                "questions": [],
                "knowledge_gained": [],
                "tools_discovered": [],
                "architecture": [],
                "code_artifacts": []
            }"#
            .to_string(),
        });
        let quota_hit = Arc::new(AtomicBool::new(false));
        let pending = PendingSession {
            idx: 0,
            total: 1,
            url: "file:///tmp/session.jsonl".to_string(),
            source: SessionSource::Codex,
            project: Some("refine".to_string()),
            captured_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
            has_embedded_timestamp: true,
            raw_content: "User: fix the ingest bug".to_string(),
            needs_chunk: false,
            chunks: Vec::new(),
            existing_document: None,
        };

        let item_count = process_single_session(&pending, &client, &doc_store, &quota_hit)
            .await
            .expect("session ingest should succeed");
        assert_eq!(item_count, 3);

        let docs = DocumentRepository::find_recent(store.as_ref(), 0, 10)
            .await
            .expect("documents should load");
        assert_eq!(docs.len(), 1);
        let saved_doc = &docs[0];
        assert_eq!(saved_doc.captured_at(), pending.captured_at);

        let linked_items = store
            .find_by_document_id(saved_doc.id())
            .await
            .expect("items should be queryable by saved document id");
        assert_eq!(linked_items.len(), item_count);
        assert!(linked_items
            .iter()
            .all(|item| item.document_id() == Some(saved_doc.id())));
    }

    #[tokio::test]
    async fn process_single_session_refresh_replaces_old_items_and_preserves_raw_transcript() {
        let store = Arc::new(SqliteStore::in_memory().expect("in-memory sqlite store"));
        let item_store: Arc<dyn ItemRepository> = store.clone();
        let doc_store: Arc<dyn DocumentRepository> = store.clone();

        let mut existing_doc = Document::new("codex-session", "old raw");
        existing_doc.set_url("file:///tmp/session.jsonl");
        doc_store
            .save(&existing_doc)
            .await
            .expect("existing document should save");

        let mut old_item = Item::new_observation("old", "old");
        old_item.set_document_id(existing_doc.id().clone());
        item_store
            .save(&old_item)
            .await
            .expect("old item should save");

        let client: Arc<dyn LlmClient> = Arc::new(StaticLlmClient {
            response: r#"{
                "session_summary": "刷新后的会话",
                "cognitive_level": "proficient",
                "collaboration_mode": "pair_programming",
                "decisions": ["保留原始 transcript"],
                "bugs_fixed": ["替换 stale session 的旧 items"],
                "patterns": [],
                "friction": [],
                "project_progress": [],
                "questions": [],
                "knowledge_gained": [],
                "tools_discovered": [],
                "architecture": [],
                "code_artifacts": []
            }"#
            .to_string(),
        });
        let quota_hit = Arc::new(AtomicBool::new(false));
        let pending = PendingSession {
            idx: 0,
            total: 1,
            url: existing_doc.url().to_string(),
            source: SessionSource::Codex,
            project: Some("refine".to_string()),
            captured_at: Utc.with_ymd_and_hms(2026, 5, 20, 12, 0, 0).unwrap(),
            has_embedded_timestamp: false,
            raw_content: "User: original transcript\nAssistant: final answer\n".to_string(),
            needs_chunk: true,
            chunks: vec!["chunk summary input".to_string()],
            existing_document: Some(existing_doc.clone()),
        };

        let item_count = process_single_session(&pending, &client, &doc_store, &quota_hit)
            .await
            .expect("session refresh should succeed");
        assert_eq!(item_count, 3);

        let saved_doc = doc_store
            .find_by_url(&pending.url)
            .await
            .expect("document query should succeed")
            .expect("document should exist");
        assert_eq!(saved_doc.id(), existing_doc.id());
        assert_eq!(saved_doc.raw_content(), pending.raw_content);
        assert_eq!(saved_doc.title(), Some("刷新后的会话"));
        assert_eq!(saved_doc.captured_at(), existing_doc.captured_at());

        let linked_items = store
            .find_by_document_id(existing_doc.id())
            .await
            .expect("items should be queryable by saved document id");
        assert_eq!(linked_items.len(), item_count);
        assert!(linked_items.iter().all(|item| item.title() != "old"));
    }

    #[tokio::test]
    async fn quota_hit_short_circuits_before_llm_call() {
        use refine_core::infra::ClaudeClient;
        let client: Arc<dyn LlmClient> = Arc::new(ClaudeClient::new("test-key"));
        let quota_hit = Arc::new(AtomicBool::new(true));

        let err = llm_call_with_retry(&client, "content", &quota_hit)
            .await
            .expect_err("quota flag should skip the call");

        assert!(err.to_string().contains("LLM 配额已耗尽"));
    }
}
