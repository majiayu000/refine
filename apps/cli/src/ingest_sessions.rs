//! ingest-sessions 命令实现
//!
//! 从 Claude Code / Codex 会话文件中提取认知观测

use anyhow::{Context, Result};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentId, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_facet_prompt, chunk_session, discover_sessions, facets_to_items, needs_chunking,
    parse_facet_response, parse_session_file, FilterConfig, SessionSource, FACET_SYSTEM_PROMPT,
};
use std::sync::Arc;

pub struct IngestOptions {
    pub source: Option<SessionSource>,
    pub limit: Option<usize>,
    pub dry_run: bool,
}

pub async fn handle_ingest_sessions(
    options: IngestOptions,
    item_store: Arc<dyn ItemRepository>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Arc<dyn LlmClient>,
) -> Result<()> {
    // 1. 发现会话文件
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
    let mut total_items = 0usize;

    for discovered_session in &sessions_to_process {
        let url = discovered_session.path.to_string_lossy().to_string();

        // 2. 去重检查
        if doc_store.find_by_url(&url).await?.is_some() {
            skipped_dup += 1;
            continue;
        }

        // 3. 解析
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

        // 4. 过滤
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

        // 5. 分块 + facet 提取
        let content = if needs_chunking(&session) {
            let chunks = chunk_session(&session);
            let mut summaries = Vec::new();
            for chunk in &chunks {
                match extract_facets_from_content(&chunk.content, &llm_client).await {
                    Ok(text) => summaries.push(text),
                    Err(e) => tracing::warn!("分块提取失败: {}", e),
                }
            }
            summaries.join("\n\n---\n\n")
        } else {
            session.to_document_content()
        };

        let facet_response = match extract_and_parse_facets(&content, &llm_client).await {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("facet 提取失败 {}: {}", url, e);
                continue;
            }
        };

        // 6. 保存 Document
        let doc_id = DocumentId::new();
        let mut doc = Document::new(discovered_session.source.as_str(), &content);
        doc.set_title(&facet_response.session_summary);
        doc.set_url(&url);
        doc_store
            .save(&doc)
            .await
            .context("保存 Document 失败")?;

        // 7. 保存 Observation Items
        let items =
            facets_to_items(&facet_response, &doc_id, discovered_session.project.as_deref());
        for item in &items {
            item_store.save(item).await.context("保存 Item 失败")?;
        }

        total_items += items.len();
        processed += 1;

        println!(
            "  + {} | {} items | {}",
            &facet_response.session_summary,
            items.len(),
            url,
        );
    }

    println!("\n完成: 处理 {}, 跳过重复 {}, 过滤 {}, 生成 {} 条观测",
        processed, skipped_dup, skipped_filter, total_items);

    Ok(())
}

async fn extract_facets_from_content(
    content: &str,
    llm_client: &Arc<dyn LlmClient>,
) -> Result<String> {
    let prompt = build_facet_prompt(content);
    let response = llm_client
        .complete(&prompt, Some(FACET_SYSTEM_PROMPT))
        .await
        .map_err(|e| anyhow::anyhow!("LLM 调用失败: {}", e))?;
    Ok(response)
}

async fn extract_and_parse_facets(
    content: &str,
    llm_client: &Arc<dyn LlmClient>,
) -> Result<refine_core::session::FacetResponse> {
    let response = extract_facets_from_content(content, llm_client).await?;
    parse_facet_response(&response).map_err(|e| anyhow::anyhow!(e))
}
