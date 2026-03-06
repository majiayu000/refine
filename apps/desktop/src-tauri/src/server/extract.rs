use super::json::parse_json_body;
use refine_core::infra::LlmClient;
use refine_core::knowledge::{DocumentId, ItemRepository, Source};
use refine_core::refinement::{extract_items_with_defaults, ExtractionPolicy, ItemExtractionInput};
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
    store: &Arc<dyn ItemRepository>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Result<Vec<String>, String> {
    let req = parse_json_body::<ExtractRequest>(request)?;
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
    store: &Arc<dyn ItemRepository>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    runtime.block_on(extract_and_store(store, llm_client.map(Arc::clone), req))
}

async fn extract_and_store(
    store: &Arc<dyn ItemRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    let source = Source::new(&req.source).with_url(&req.url);
    let input = ItemExtractionInput {
        source: &req.source,
        title: req.title.as_deref(),
        raw_content: &req.content,
        captured_at: None,
        policy: ExtractionPolicy::default(),
    };
    let doc_id = DocumentId::new();
    let items = extract_items_with_defaults(llm_client.as_deref(), &input, &source, &doc_id).await;

    let mut ids = Vec::with_capacity(items.len());
    for item in &items {
        store.save(item).await.map_err(|e| e.to_string())?;
        ids.push(item.id().to_string());
    }

    Ok(ids)
}
