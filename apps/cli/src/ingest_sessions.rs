//! ingest-sessions 命令实现
//!
//! 支持 3 路并发 + 断点续传 + API 限流重试

use anyhow::{Context, Result};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentId, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_facet_prompt, chunk_session, discover_sessions, facets_to_items, needs_chunking,
    parse_facet_response, parse_session_file, FilterConfig, SessionSource, FACET_SYSTEM_PROMPT,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Semaphore;

const MAX_RETRIES: usize = 5;
const RETRY_BASE_DELAY_SECS: u64 = 10;
const CONCURRENCY: usize = 3;

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
    content: String,
    needs_chunk: bool,
    chunks: Vec<String>,
}

/// Read the Unix-second timestamp from the scoped ingest cursor file.
fn read_last_ingest_mtime(db_path: &Path, source: Option<&SessionSource>) -> Option<SystemTime> {
    let home = dirs::home_dir()?;
    read_last_ingest_mtime_in(&home, db_path, source)
}

fn read_last_ingest_mtime_in(
    home: &Path,
    db_path: &Path,
    source: Option<&SessionSource>,
) -> Option<SystemTime> {
    let path = last_ingest_mtime_path(home, db_path, source);
    let secs: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

/// Persist the Unix-second timestamp to the scoped ingest cursor file.
fn write_last_ingest_mtime(db_path: &Path, source: Option<&SessionSource>, t: SystemTime) {
    let Some(home) = dirs::home_dir() else { return };
    write_last_ingest_mtime_in(&home, db_path, source, t);
}

fn write_last_ingest_mtime_in(
    home: &Path,
    db_path: &Path,
    source: Option<&SessionSource>,
    t: SystemTime,
) {
    let path = last_ingest_mtime_path(home, db_path, source);
    let Some(dir) = path.parent() else { return };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("failed to create ~/.refine: {}", e);
        return;
    }
    if let Ok(dur) = t.duration_since(SystemTime::UNIX_EPOCH) {
        if let Err(e) = std::fs::write(&path, dur.as_secs().to_string()) {
            tracing::warn!("failed to write last-ingest-mtime: {}", e);
        }
    }
}

fn last_ingest_mtime_path(home: &Path, db_path: &Path, source: Option<&SessionSource>) -> PathBuf {
    let source_key = source.map_or("all", ingest_source_key);
    let db_key = hex_encode(authoritative_db_path(db_path).to_string_lossy().as_bytes());
    home.join(".refine")
        .join(format!("last-ingest-mtime-{source_key}-{db_key}"))
}

fn ingest_source_key(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::ClaudeCode => "claude-code",
        SessionSource::Codex => "codex",
    }
}

fn authoritative_db_path(db_path: &Path) -> PathBuf {
    if let Ok(path) = std::fs::canonicalize(db_path) {
        return path;
    }
    if db_path.is_absolute() {
        return db_path.to_path_buf();
    }
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(db_path)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub async fn handle_ingest_sessions(
    options: IngestOptions,
    db_path: &Path,
    item_store: Arc<dyn ItemRepository>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    // Incremental scan: only active for full (no --limit/--latest) non-dry-run runs.
    // The cursor is partitioned by source filter and authoritative DB path.
    // 1-hour overlap on the cutoff absorbs clock skew and files modified near the boundary.
    let incremental = options.limit.is_none() && options.latest.is_none() && !options.dry_run;
    let scan_start = SystemTime::now();
    let mtime_after = if incremental {
        read_last_ingest_mtime(db_path, options.source.as_ref()).map(|last| {
            last.checked_sub(Duration::from_secs(3600))
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
    } else {
        None
    };

    let mut discovered = discover_sessions(options.source.clone(), mtime_after);
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

    // 阶段 1: 串行做去重 + 过滤 + 解析（快，不需要 LLM）
    for (idx, ds) in sessions_to_process.iter().enumerate() {
        let url = ds.path.to_string_lossy().to_string();

        if doc_store.find_by_url(&url).await?.is_some() {
            skipped_dup += 1;
            continue;
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

        let (content, chunks) = if needs_chunking(&session) {
            let cs = chunk_session(&session);
            let chunk_texts: Vec<String> = cs.iter().map(|c| c.content.clone()).collect();
            (String::new(), chunk_texts)
        } else {
            (session.to_document_content(), Vec::new())
        };

        pending.push(PendingSession {
            idx,
            total,
            url,
            source: ds.source.clone(),
            project: ds.project.clone(),
            content,
            needs_chunk: !chunks.is_empty(),
            chunks,
        });
    }

    if options.dry_run {
        let dry_count = total - skipped_dup - skipped_filter;
        println!(
            "\n[dry-run] 可处理 {}, 跳过重复 {}, 过滤 {}",
            dry_count, skipped_dup, skipped_filter
        );
        return Ok(());
    }

    println!(
        "待处理 {} 个会话（跳过重复 {}, 过滤 {}），{} 路并发...\n",
        pending.len(),
        skipped_dup,
        skipped_filter,
        CONCURRENCY,
    );

    if pending.is_empty() {
        println!("全部已处理完毕。");
        return Ok(());
    }

    // 阶段 2: 并发做 LLM 提取 + 保存
    let client = llm_client.ok_or_else(|| anyhow::anyhow!("非 dry-run 模式需要 LLM API Key"))?;
    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));
    let processed = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let total_items = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();

    for ps in pending {
        let sem = semaphore.clone();
        let client = client.clone();
        let item_store = item_store.clone();
        let doc_store = doc_store.clone();
        let processed = processed.clone();
        let failed = failed.clone();
        let total_items = total_items.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");

            let result = process_single_session(&ps, &client, &item_store, &doc_store).await;

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
        "\n完成: 处理 {}, 跳过重复 {}, 过滤 {}, 失败 {}, 生成 {} 条观测",
        processed, skipped_dup, skipped_filter, failed, total_items
    );
    if failed > 0 {
        eprintln!("提示: 重新运行即可续传失败的会话");
        return Err(anyhow::anyhow!("{} 个会话提取失败", failed));
    }

    // Advance the incremental scan cursor so the next run only sees newer files.
    if incremental {
        write_last_ingest_mtime(db_path, options.source.as_ref(), scan_start);
    }

    Ok(())
}

async fn process_single_session(
    ps: &PendingSession,
    client: &Arc<dyn LlmClient>,
    item_store: &Arc<dyn ItemRepository>,
    doc_store: &Arc<dyn DocumentRepository>,
) -> Result<usize> {
    let content = if ps.needs_chunk {
        let mut summaries = Vec::new();
        for chunk in &ps.chunks {
            match llm_call_with_retry(client, chunk).await {
                Ok(text) => summaries.push(text),
                Err(e) => tracing::warn!("分块提取失败: {}", e),
            }
        }
        summaries.join("\n\n---\n\n")
    } else {
        ps.content.clone()
    };

    let facet_response = extract_and_parse_facets_with_retry(&content, client).await?;

    let doc_id = DocumentId::new();
    let mut doc = Document::new(ps.source.as_str(), &content);
    doc.set_title(&facet_response.session_summary);
    doc.set_url(&ps.url);
    doc_store.save(&doc).await.context("保存 Document 失败")?;

    let items = facets_to_items(&facet_response, &doc_id, ps.project.as_deref());
    let item_count = items.len();
    for item in &items {
        item_store.save(item).await.context("保存 Item 失败")?;
    }

    println!(
        "  + [{}/{}] {} | {} items",
        ps.idx + 1,
        ps.total,
        &facet_response.session_summary,
        item_count,
    );

    Ok(item_count)
}

async fn llm_call_with_retry(client: &Arc<dyn LlmClient>, content: &str) -> Result<String> {
    let prompt = build_facet_prompt(content);
    let mut last_err = String::new();

    for attempt in 0..MAX_RETRIES {
        match client.complete(&prompt, Some(FACET_SYSTEM_PROMPT)).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_err = e.to_string();
                let is_retryable = last_err.contains("cooldown")
                    || last_err.contains("service_busy")
                    || last_err.contains("rate")
                    || last_err.contains("429")
                    || last_err.contains("Upstream")
                    || last_err.contains("timeout")
                    || last_err.contains("empty response");

                if !is_retryable || attempt == MAX_RETRIES - 1 {
                    break;
                }

                let delay = RETRY_BASE_DELAY_SECS * (1 << attempt);
                eprintln!(
                    "    ⏳ 重试 ({}/{}) 等待 {}s: {}",
                    attempt + 1,
                    MAX_RETRIES,
                    delay,
                    &last_err[..last_err.len().min(80)],
                );
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
    }

    Err(anyhow::anyhow!(
        "LLM 调用失败 (重试 {} 次): {}",
        MAX_RETRIES,
        last_err
    ))
}

async fn extract_and_parse_facets_with_retry(
    content: &str,
    client: &Arc<dyn LlmClient>,
) -> Result<refine_core::session::FacetResponse> {
    let response = llm_call_with_retry(client, content).await?;
    parse_facet_response(&response).map_err(|e| anyhow::anyhow!(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::session::discover_sessions_in;
    use std::fs;

    fn write_session(path: &Path, mtime_secs: i64) {
        fs::create_dir_all(path.parent().expect("session parent")).unwrap();
        fs::write(path, "{}").unwrap();
        filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(mtime_secs, 0)).unwrap();
    }

    fn overlap_cutoff(last: SystemTime) -> SystemTime {
        last.checked_sub(Duration::from_secs(3600))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn source_scoped_cursor_does_not_hide_other_source_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let db_path = home.join("data").join("refine.db");
        let claude_cursor = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);

        write_last_ingest_mtime_in(
            home,
            &db_path,
            Some(&SessionSource::ClaudeCode),
            claude_cursor,
        );
        write_session(&home.join(".codex/sessions/2026/codex.jsonl"), 1_000);

        assert_eq!(
            read_last_ingest_mtime_in(home, &db_path, Some(&SessionSource::ClaudeCode)),
            Some(claude_cursor)
        );
        assert_eq!(
            read_last_ingest_mtime_in(home, &db_path, Some(&SessionSource::Codex)),
            None
        );

        let codex_results = discover_sessions_in(
            home,
            Some(SessionSource::Codex),
            read_last_ingest_mtime_in(home, &db_path, Some(&SessionSource::Codex))
                .map(overlap_cutoff),
        );
        assert_eq!(codex_results.len(), 1);
        assert_eq!(codex_results[0].source, SessionSource::Codex);
    }

    #[test]
    fn cursor_is_partitioned_by_db_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let db_a = home.join("dbs").join("a.db");
        let db_b = home.join("dbs").join("b.db");
        let cursor = SystemTime::UNIX_EPOCH + Duration::from_secs(12_345);

        write_last_ingest_mtime_in(home, &db_a, Some(&SessionSource::Codex), cursor);

        assert_eq!(
            read_last_ingest_mtime_in(home, &db_a, Some(&SessionSource::Codex)),
            Some(cursor)
        );
        assert_eq!(
            read_last_ingest_mtime_in(home, &db_b, Some(&SessionSource::Codex)),
            None
        );
    }
}
