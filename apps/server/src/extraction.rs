use refine_core::knowledge::{Document, DocumentId, DocumentRepository, Item, ItemId, Source};
use refine_core::refinement::{
    extract_items_with_strict_defaults, ExtractionPolicy, ItemExtractionInput,
};
use std::sync::Arc;

use crate::models::{
    now_iso, ConversationRecord, ConversationStatus, ExtractionJobRecord, ExtractionMode, JobStatus,
};
use crate::state::AppState;

pub fn spawn_extraction(
    state: Arc<AppState>,
    conversation_id: String,
    job_id: String,
    mode: ExtractionMode,
) {
    tokio::spawn(async move {
        if let Err(err) = run_extraction(state.clone(), &conversation_id, &job_id, mode).await {
            set_conversation_failed(&state, &conversation_id, &err).await;
            set_job_failed(&state, &job_id, &err).await;
        }
    });
}

async fn run_extraction(
    state: Arc<AppState>,
    conversation_id: &str,
    job_id: &str,
    mode: ExtractionMode,
) -> Result<(), String> {
    set_job_running(&state, job_id).await;
    set_conversation_status(&state, conversation_id, ConversationStatus::Processing).await;

    let conversation = state
        .conversation_repo
        .find_conversation_by_id(conversation_id)?
        .ok_or_else(|| "Conversation not found".to_string())?;

    let source = Source::new(&conversation.source).with_url(&conversation.url);

    let mut doc = Document::new(&conversation.source, &conversation.raw_content);
    doc.set_url(&conversation.url);
    if let Some(title) = &conversation.title {
        doc.set_title(title);
    }
    let doc_id = save_document(&state.doc_store, &doc).await?;

    let input = ItemExtractionInput {
        source: &conversation.source,
        title: conversation.title.as_deref(),
        raw_content: &conversation.raw_content,
        captured_at: Some(&conversation.captured_at),
        policy: mode_to_policy(mode),
    };
    let llm_client = state
        .llm_client
        .as_deref()
        .ok_or_else(|| "LLM client is required for strict extraction mode".to_string())?;
    let items = extract_items_with_strict_defaults(llm_client, &input, &source, &doc_id)
        .await
        .map_err(|e| e.to_string())?;

    let item_ids = save_and_index_items(&state, &items).await?;

    set_conversation_processed(&state, conversation_id, item_ids).await;
    set_job_succeeded(&state, job_id).await;

    Ok(())
}

async fn save_and_index_items(
    state: &Arc<AppState>,
    items: &[Item],
) -> Result<Vec<String>, String> {
    let mut saved_ids = Vec::with_capacity(items.len());
    let mut indexed_ids = Vec::with_capacity(items.len());

    for item in items {
        if let Err(err) = state.store.save(item).await {
            let cleanup_error = cleanup_partial_items(state, &saved_ids, &indexed_ids).await;
            return Err(with_cleanup_error(
                format!("failed to save item {}: {}", item.id(), err),
                cleanup_error,
            ));
        }
        saved_ids.push(item.id().clone());

        if let Err(err) = state.engine.index_item(item).await {
            let cleanup_error = cleanup_partial_items(state, &saved_ids, &indexed_ids).await;
            return Err(with_cleanup_error(
                format!(
                    "failed to index item {} for semantic search: {}",
                    item.id(),
                    err
                ),
                cleanup_error,
            ));
        }
        indexed_ids.push(item.id().clone());
    }

    Ok(saved_ids.iter().map(ToString::to_string).collect())
}

async fn cleanup_partial_items(
    state: &Arc<AppState>,
    saved_ids: &[ItemId],
    indexed_ids: &[ItemId],
) -> Option<String> {
    let mut errors = Vec::new();

    for item_id in indexed_ids {
        if let Err(err) = state.engine.remove_from_index(item_id.as_str()).await {
            errors.push(format!("remove index {}: {}", item_id, err));
        }
    }

    for item_id in saved_ids {
        match state.store.delete(item_id).await {
            Ok(true) => {}
            Ok(false) => errors.push(format!("delete item {}: not found", item_id)),
            Err(err) => errors.push(format!("delete item {}: {}", item_id, err)),
        }
    }

    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

fn with_cleanup_error(error: String, cleanup_error: Option<String>) -> String {
    match cleanup_error {
        Some(cleanup_error) => format!("{}; cleanup failed: {}", error, cleanup_error),
        None => error,
    }
}

fn mode_to_policy(mode: ExtractionMode) -> ExtractionPolicy {
    match mode {
        ExtractionMode::Auto => ExtractionPolicy::default(),
        ExtractionMode::Knowledge => ExtractionPolicy::knowledge_only(),
        ExtractionMode::Snippet => ExtractionPolicy::snippets_only(),
        ExtractionMode::Skill => ExtractionPolicy {
            extract_knowledge: false,
            extract_skills: true,
            extract_snippets: false,
            ..ExtractionPolicy::default()
        },
    }
}

async fn set_job_running(state: &Arc<AppState>, job_id: &str) {
    update_job(
        state,
        job_id,
        |job| {
            job.status = JobStatus::Running;
            job.updated_at = now_iso();
            job.error = None;
        },
        "running",
    )
    .await;
}

async fn set_job_succeeded(state: &Arc<AppState>, job_id: &str) {
    update_job(
        state,
        job_id,
        |job| {
            job.status = JobStatus::Succeeded;
            job.updated_at = now_iso();
            job.error = None;
        },
        "succeeded",
    )
    .await;
}

async fn set_job_failed(state: &Arc<AppState>, job_id: &str, error: &str) {
    update_job(
        state,
        job_id,
        |job| {
            job.status = JobStatus::Failed;
            job.updated_at = now_iso();
            job.error = Some(error.to_string());
        },
        "failed",
    )
    .await;
}

async fn set_conversation_status(
    state: &Arc<AppState>,
    conversation_id: &str,
    status: ConversationStatus,
) {
    update_conversation(
        state,
        conversation_id,
        move |conversation| {
            conversation.status = status;
        },
        "status",
    )
    .await;
}

async fn set_conversation_processed(
    state: &Arc<AppState>,
    conversation_id: &str,
    item_ids: Vec<String>,
) {
    update_conversation(
        state,
        conversation_id,
        move |conversation| {
            conversation.status = ConversationStatus::Processed;
            conversation.item_ids = item_ids;
            conversation.last_error = None;
        },
        "processed",
    )
    .await;
}

async fn set_conversation_failed(state: &Arc<AppState>, conversation_id: &str, error: &str) {
    update_conversation(
        state,
        conversation_id,
        |conversation| {
            conversation.status = ConversationStatus::Failed;
            conversation.last_error = Some(error.to_string());
        },
        "failed",
    )
    .await;
}

async fn update_job<F>(state: &Arc<AppState>, job_id: &str, mutate: F, phase: &str)
where
    F: FnOnce(&mut ExtractionJobRecord),
{
    let mut job = match state.job_repo.find_job_by_id(job_id) {
        Ok(Some(job)) => job,
        Ok(None) => {
            tracing::warn!("persist {} job skipped: not found {}", phase, job_id);
            return;
        }
        Err(err) => {
            tracing::warn!("persist {} job skipped: load error {}", phase, err);
            return;
        }
    };
    mutate(&mut job);
    if let Err(err) = state.job_repo.upsert_job(&job) {
        tracing::warn!("persist {} job failed: {}", phase, err);
    }
}

async fn save_document(
    doc_store: &Arc<dyn DocumentRepository>,
    doc: &Document,
) -> Result<DocumentId, String> {
    doc_store
        .save(doc)
        .await
        .map_err(|e| format!("failed to save document: {}", e))?;
    doc_store
        .find_by_url(doc.url())
        .await
        .map_err(|e| format!("failed to find document by url after save: {}", e))?
        .map(|d| d.id().clone())
        .ok_or_else(|| "document not found after save — unexpected state".to_string())
}

async fn update_conversation<F>(
    state: &Arc<AppState>,
    conversation_id: &str,
    mutate: F,
    phase: &str,
) where
    F: FnOnce(&mut ConversationRecord),
{
    let mut conversation = match state
        .conversation_repo
        .find_conversation_by_id(conversation_id)
    {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            tracing::warn!(
                "persist {} conversation skipped: not found {}",
                phase,
                conversation_id
            );
            return;
        }
        Err(err) => {
            tracing::warn!("persist {} conversation skipped: load error {}", phase, err);
            return;
        }
    };
    mutate(&mut conversation);
    if let Err(err) = state.conversation_repo.upsert_conversation(&conversation) {
        tracing::warn!("persist {} conversation failed: {}", phase, err);
    }
}

#[cfg(test)]
mod tests {
    use super::spawn_extraction;
    use crate::models::{
        now_iso, ConversationRecord, ConversationStatus, ExtractionJobRecord, ExtractionMode,
        JobStatus,
    };
    use crate::persistence::ServerPersistence;
    use crate::state::AppState;
    use async_trait::async_trait;
    use refine_core::error::{InfraError, InfraResult};
    use refine_core::infra::{LlmClient, SqliteStore};
    use refine_core::knowledge::{DocumentRepository, ItemRepository};
    use refine_core::search::{SearchEngine, VectorSearch};
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration, Instant};
    use uuid::Uuid;

    struct StaticLlmClient;

    #[async_trait]
    impl LlmClient for StaticLlmClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            Ok(
                r#"{"items":[{"type":"knowledge","title":"Indexed","summary":"S","content":"C","tags":[]}]}"#
                    .to_string(),
            )
        }
    }

    struct FailingVectorSearch;

    #[async_trait]
    impl VectorSearch for FailingVectorSearch {
        async fn search(&self, _query: &str, _limit: usize) -> InfraResult<Vec<(String, f32)>> {
            Ok(Vec::new())
        }

        async fn index(&self, _id: &str, _text: &str) -> InfraResult<()> {
            Err(InfraError::Database("vector index unavailable".to_string()))
        }

        async fn remove(&self, _id: &str) -> InfraResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn semantic_index_failure_marks_extraction_failed() {
        let (_tmp, state, persistence, conversation_id, job_id) = build_state_with_failing_index()
            .await
            .expect("build test state");

        spawn_extraction(
            state.clone(),
            conversation_id.clone(),
            job_id.clone(),
            ExtractionMode::Auto,
        );

        let job = wait_for_failed_job(&persistence, &job_id).await;
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("failed to index item"));

        let conversation = persistence
            .find_conversation_by_id(&conversation_id)
            .expect("load conversation")
            .expect("conversation exists");
        assert_eq!(conversation.status, ConversationStatus::Failed);
        assert!(conversation.item_ids.is_empty());
        assert!(conversation
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("failed to index item"));

        let saved_items = match state.store.find_all().await {
            Ok(items) => items,
            Err(err) => panic!("load saved items: {}", err),
        };
        assert!(
            saved_items.is_empty(),
            "failed extraction left orphan items: {:?}",
            saved_items
        );
    }

    async fn build_state_with_failing_index() -> Result<
        (
            TempDir,
            Arc<AppState>,
            Arc<ServerPersistence>,
            String,
            String,
        ),
        String,
    > {
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let db_path = tmp.path().join("refine-server.sqlite");
        let persistence = Arc::new(ServerPersistence::new(db_path.clone())?);
        let sqlite_store = Arc::new(SqliteStore::open(&db_path).map_err(|e| e.to_string())?);
        let store: Arc<dyn ItemRepository> = sqlite_store.clone();
        let doc_store: Arc<dyn DocumentRepository> = sqlite_store;
        let engine = Arc::new(
            SearchEngine::new(store.clone()).with_vector_search(Arc::new(FailingVectorSearch)),
        );

        let conversation_id = Uuid::new_v4().to_string();
        let job_id = Uuid::new_v4().to_string();
        let now = now_iso();
        persistence.upsert_conversation(&ConversationRecord {
            id: conversation_id.clone(),
            user_id: "test-user".to_string(),
            source: "test".to_string(),
            url: format!("https://example.com/{}", conversation_id),
            title: Some("test conversation".to_string()),
            raw_content: "Human: hello\nAssistant: world".to_string(),
            metadata: json!({}),
            captured_at: now.clone(),
            created_at: now.clone(),
            status: ConversationStatus::Queued,
            idempotency_key: Uuid::new_v4().to_string(),
            item_ids: Vec::new(),
            last_error: None,
        })?;
        persistence.upsert_job(&ExtractionJobRecord {
            id: job_id.clone(),
            conversation_id: conversation_id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
        })?;

        let state = Arc::new(AppState {
            store,
            doc_store,
            engine,
            semantic_search_enabled: true,
            free_quota_items: 0,
            premium_users: Default::default(),
            llm_client: Some(Arc::new(StaticLlmClient)),
            api_token: None,
            dev_anon: true,
            conversation_repo: persistence.clone(),
            job_repo: persistence.clone(),
            event_repo: persistence.clone(),
        });

        Ok((tmp, state, persistence, conversation_id, job_id))
    }

    async fn wait_for_failed_job(
        persistence: &ServerPersistence,
        job_id: &str,
    ) -> ExtractionJobRecord {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let job = persistence
                .find_job_by_id(job_id)
                .expect("load job")
                .expect("job exists");
            if job.status == JobStatus::Failed {
                return job;
            }
            assert!(
                Instant::now() < deadline,
                "job did not fail before timeout; latest status: {:?}",
                job.status
            );
            sleep(Duration::from_millis(20)).await;
        }
    }
}
