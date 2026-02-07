use refine_core::infra::{build_llm_client_from_env as build_core_llm_client_from_env, LlmClient, SqliteStore};
use refine_core::knowledge::{Item, ItemRepository, Source};
use refine_core::refinement::{build_fallback_item, extract_items_with_llm, ExtractionPolicy};
use serde::Deserialize;
use std::sync::Arc;
use tokio::runtime::Runtime;

#[derive(Debug, Clone)]
pub(super) struct IngestRequest {
    pub content: String,
    pub url: String,
    pub source: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExtractRequest {
    content: String,
    url: String,
    source: String,
    title: Option<String>,
}

pub(super) fn handle_extract(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Result<Vec<String>, String> {
    let req = parse_request_body(request)?;
    ingest_conversation(
        store,
        runtime,
        llm_client,
        IngestRequest {
            content: req.content,
            url: req.url,
            source: req.source,
            title: req.title,
        },
    )
}

pub(super) fn ingest_conversation(
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    runtime.block_on(extract_and_store(store, llm_client.map(Arc::clone), req))
}

pub(super) fn build_llm_client_from_env() -> Result<Arc<dyn LlmClient>, String> {
    build_core_llm_client_from_env().ok_or_else(|| "missing API key".to_string())
}

fn parse_request_body(request: &mut tiny_http::Request) -> Result<ExtractRequest, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|_| "Failed to read request body".to_string())?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON: {}", e))
}

async fn extract_and_store(
    store: &Arc<SqliteStore>,
    llm_client: Option<Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    let items = build_items(llm_client, &req).await;

    let mut ids = Vec::with_capacity(items.len());
    for mut item in items {
        item.set_source(Source::new(&req.source).with_url(&req.url));
        if item.content().trim().is_empty() {
            item.set_content(&req.content);
        }
        store.save(&item).await.map_err(|e| e.to_string())?;
        ids.push(item.id().to_string());
    }

    Ok(ids)
}

async fn build_items(llm_client: Option<Arc<dyn LlmClient>>, req: &IngestRequest) -> Vec<Item> {
    let fallback = || build_fallback_item(&req.source, req.title.as_deref(), &req.content, None);

    let Some(client) = llm_client else {
        return vec![fallback()];
    };

    match extract_items_with_llm(client.as_ref(), &req.content, ExtractionPolicy::default()).await {
        Ok(items) => items,
        Err(err) => {
            tracing::warn!("提炼失败，降级为 fallback item: {}", err);
            vec![fallback()]
        }
    }
}
