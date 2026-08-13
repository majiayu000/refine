use refine_core::infra::LlmClient;
use refine_core::knowledge::{DocumentRepository, Source};
use refine_core::refinement::{
    extract_document_with_strict_defaults, persist_extracted_document, ExtractionPolicy,
    ItemExtractionInput,
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
    doc_store: &Arc<dyn DocumentRepository>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    runtime.block_on(extract_and_store(
        doc_store,
        llm_client.map(Arc::clone),
        req,
    ))
}

async fn extract_and_store(
    doc_store: &Arc<dyn DocumentRepository>,
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
    let aggregate = extract_document_with_strict_defaults(llm_client, &input, &source)
        .await
        .map_err(|e| e.to_string())?;

    let ids = persist_extracted_document(doc_store.as_ref(), &aggregate)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ids)
}
