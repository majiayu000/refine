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
    pub captured_at: Option<String>,
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
        captured_at: req.captured_at.as_deref(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use refine_core::error::InfraResult;
    use refine_core::infra::SqliteStore;
    use refine_core::knowledge::{DocumentRepository, ItemRepository};
    use tempfile::tempdir;

    struct FakeLlm;

    #[async_trait]
    impl LlmClient for FakeLlm {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            Ok(r#"{"items":[{"type":"knowledge","title":"Desktop item","summary":"S","content":"C","tags":[]}]}"#.to_string())
        }
    }

    fn request(captured_at: Option<&str>) -> IngestRequest {
        IngestRequest {
            content: "Human: hello\nAssistant: world".to_string(),
            url: "https://example.test/conversation".to_string(),
            source: "extension".to_string(),
            title: Some("Conversation".to_string()),
            captured_at: captured_at.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn desktop_ingest_persists_event_time_and_reuses_url_identity() {
        let temp = tempdir().unwrap();
        let sqlite = Arc::new(SqliteStore::open(&temp.path().join("refine.db")).unwrap());
        let doc_store: Arc<dyn DocumentRepository> = sqlite.clone();
        let item_store: Arc<dyn ItemRepository> = sqlite;
        let runtime = Runtime::new().unwrap();
        let llm: Arc<dyn LlmClient> = Arc::new(FakeLlm);

        ingest_conversation(
            &doc_store,
            &runtime,
            Some(&llm),
            request(Some("2020-01-02T03:04:05Z")),
        )
        .unwrap();
        ingest_conversation(
            &doc_store,
            &runtime,
            Some(&llm),
            request(Some("2020-01-02T03:04:05Z")),
        )
        .unwrap();

        let doc = runtime
            .block_on(doc_store.find_by_url("https://example.test/conversation"))
            .unwrap()
            .unwrap();
        assert_eq!(doc.captured_at().to_rfc3339(), "2020-01-02T03:04:05+00:00");
        assert_eq!(runtime.block_on(doc_store.count()).unwrap(), 1);
        assert_eq!(runtime.block_on(item_store.count_items(None)).unwrap(), 1);
    }

    #[test]
    fn desktop_ingest_rejects_invalid_event_time_without_persisting() {
        let temp = tempdir().unwrap();
        let sqlite = Arc::new(SqliteStore::open(&temp.path().join("refine.db")).unwrap());
        let doc_store: Arc<dyn DocumentRepository> = sqlite.clone();
        let runtime = Runtime::new().unwrap();
        let llm: Arc<dyn LlmClient> = Arc::new(FakeLlm);

        let err = ingest_conversation(
            &doc_store,
            &runtime,
            Some(&llm),
            request(Some("not-a-timestamp")),
        )
        .expect_err("invalid captured_at must be rejected");

        assert!(err.contains("invalid captured_at"));
        assert_eq!(runtime.block_on(doc_store.count()).unwrap(), 0);
    }
}
