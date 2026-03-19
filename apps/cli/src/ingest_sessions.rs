//! ingest-sessions 命令实现
//!
//! 从 Claude Code / Codex 会话文件中提取认知观测
//! 支持断点续传（find_by_url 去重）和 API 限流重试

use anyhow::{Context, Result};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentId, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_facet_prompt, chunk_session, discover_sessions, facets_to_items, needs_chunking,
    parse_facet_response, parse_session_file, FilterConfig, SessionSource, FACET_SYSTEM_PROMPT,
};
use std::sync::Arc;
use std::time::Duration;

const MAX_RETRIES: usize = 3;
const RETRY_BASE_DELAY_SECS: u64 = 15;

pub struct IngestOptions {
    pub source: Option<SessionSource>,
    pub limit: Option<usize>,
    pub dry_run: bool,
}

pub async fn handle_ingest_sessions(
    options: IngestOptions,
    item_store: Arc<dyn ItemRepository>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    let discovered = discover_sessions(options.source);
    println!("发现 {} 个会话文件", discovered.len());

    let sessions_to_process: Vec<_> = match options.limit {
        Some(limit) => discovered.into_iter().take(limit).collect(),
        None => discovered,
    };

    let filter_config = FilterConfig::default();
    let mut processed = 0usize;
    let mut skipped_dup = 0usize;
    let mut skipped_filter = 0usize;
    let mut failed = 0usize;
    let mut total_items = 0usize;

    for (idx, discovered_session) in sessions_to_process.iter().enumerate() {
        let url = discovered_session.path.to_string_lossy().to_string();

        // 去重检查
        if doc_store.find_by_url(&url).await?.is_some() {
            skipped_dup += 1;
            continue;
        }

        // 解析
        let session = match parse_session_file(
            &discovered_session.path,
            discovered_session.source.clone(),
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("解析失败 {}: {}", url, e);
                continue;
            }
        };

        // 过滤
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
                discovered_session.source,
            );
            processed += 1;
            continue;
        }

        // 分块 + facet 提取（带重试）
        let client = llm_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("非 dry-run 模式需要 LLM API Key"))?;

        let content = if needs_chunking(&session) {
            let chunks = chunk_session(&session);
            let mut summaries = Vec::new();
            for chunk in &chunks {
                match llm_call_with_retry(client, &chunk.content).await {
                    Ok(text) => summaries.push(text),
                    Err(e) => tracing::warn!("分块提取失败: {}", e),
                }
            }
            summaries.join("\n\n---\n\n")
        } else {
            session.to_document_content()
        };

        let facet_response = match extract_and_parse_facets_with_retry(&content, client).await {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "  ✗ [{}/{}] facet 提取失败，跳过: {}",
                    idx + 1,
                    sessions_to_process.len(),
                    e
                );
                failed += 1;
                continue;
            }
        };

        // 保存 Document
        let doc_id = DocumentId::new();
        let mut doc = Document::new(discovered_session.source.as_str(), &content);
        doc.set_title(&facet_response.session_summary);
        doc.set_url(&url);
        doc_store
            .save(&doc)
            .await
            .context("保存 Document 失败")?;

        // 保存 Observation Items
        let items =
            facets_to_items(&facet_response, &doc_id, discovered_session.project.as_deref());
        for item in &items {
            item_store.save(item).await.context("保存 Item 失败")?;
        }

        total_items += items.len();
        processed += 1;

        println!(
            "  + [{}/{}] {} | {} items",
            idx + 1,
            sessions_to_process.len(),
            &facet_response.session_summary,
            items.len(),
        );
    }

    println!(
        "\n完成: 处理 {}, 跳过重复 {}, 过滤 {}, 失败 {}, 生成 {} 条观测",
        processed, skipped_dup, skipped_filter, failed, total_items
    );
    if skipped_dup > 0 || failed > 0 {
        println!("提示: 重新运行即可续传（已处理的会自动跳过）");
    }

    Ok(())
}

/// LLM 调用 + 指数退避重试
async fn llm_call_with_retry(
    client: &Arc<dyn LlmClient>,
    content: &str,
) -> Result<String> {
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
                    || last_err.contains("timeout");

                if !is_retryable || attempt == MAX_RETRIES - 1 {
                    break;
                }

                let delay = RETRY_BASE_DELAY_SECS * (1 << attempt);
                eprintln!(
                    "    ⏳ API 限流，等待 {}s 后重试 ({}/{})...",
                    delay,
                    attempt + 1,
                    MAX_RETRIES
                );
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }
    }

    Err(anyhow::anyhow!("LLM 调用失败 (重试 {} 次): {}", MAX_RETRIES, last_err))
}

async fn extract_and_parse_facets_with_retry(
    content: &str,
    client: &Arc<dyn LlmClient>,
) -> Result<refine_core::session::FacetResponse> {
    let response = llm_call_with_retry(client, content).await?;
    parse_facet_response(&response).map_err(|e| anyhow::anyhow!(e))
}
