use refine_core::infra::LlmClient;
use refine_core::knowledge::{DocumentId, ItemRepository, Source};
use refine_core::refinement::{
    extract_items_with_strict_defaults, ExtractionPolicy, ItemExtractionInput,
};
use std::sync::Arc;
use tokio::runtime::Runtime;

#[derive(Debug, Clone)]
pub(super) struct IngestRequest {
    pub content: String,
    pub url: String,
    pub source: String,
    pub title: Option<String>,
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
    let llm_client = llm_client
        .as_deref()
        .ok_or_else(|| "LLM client is required for strict extraction mode".to_string())?;
    let source = Source::new(&req.source).with_url(&req.url);
    let input = ItemExtractionInput {
        source: &req.source,
        title: req.title.as_deref(),
        raw_content: &req.content,
        captured_at: None,
        policy: ExtractionPolicy::default(),
    };
    let doc_id = DocumentId::new();
    let items = extract_items_with_strict_defaults(llm_client, &input, &source, &doc_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut ids = Vec::with_capacity(items.len());
    for item in &items {
        store.save(item).await.map_err(|e| e.to_string())?;
        ids.push(item.id().to_string());
    }

    Ok(ids)
}
