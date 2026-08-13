use refine_core::knowledge::{
    Document, DocumentRepository, Item, ItemId, RestoreDocumentParams, Source,
};
use refine_core::refinement::{
    extract_items_with_strict_defaults, ExtractionPolicy, ItemExtractionInput,
};
use std::collections::HashSet;
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
            if let Err(persist_err) = set_conversation_failed(&state, &conversation_id, &err).await
            {
                tracing::warn!("persist failed conversation failed: {}", persist_err);
            }
            if let Err(persist_err) = set_job_failed(&state, &job_id, &err).await {
                tracing::warn!("persist failed job failed: {}", persist_err);
            }
        }
    });
}

async fn run_extraction(
    state: Arc<AppState>,
    conversation_id: &str,
    job_id: &str,
    mode: ExtractionMode,
) -> Result<(), String> {
    set_job_running(&state, job_id).await?;
    set_conversation_status(&state, conversation_id, ConversationStatus::Processing).await?;

    let conversation = state
        .conversation_repo
        .find_conversation_by_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;

    let source = Source::new(&conversation.source).with_url(&conversation.url);

    let mut doc = Document::new(&conversation.source, &conversation.raw_content);
    doc.set_url(&conversation.url);
    if let Some(title) = &conversation.title {
        doc.set_title(title);
    }
    let captured_at = chrono::DateTime::parse_from_rfc3339(&conversation.captured_at)
        .map_err(|err| format!("invalid conversation captured_at: {err}"))?
        .with_timezone(&chrono::Utc);
    doc.set_captured_at(captured_at);
    let doc = canonicalize_document(&state.doc_store, doc).await?;
    let doc_id = doc.id().clone();

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

    let item_ids = save_document_items_and_index(&state, &doc, &items).await?;

    set_conversation_processed(&state, conversation_id, item_ids).await?;
    set_job_succeeded(&state, job_id).await?;

    Ok(())
}

async fn save_document_items_and_index(
    state: &Arc<AppState>,
    doc: &Document,
    items: &[Item],
) -> Result<Vec<String>, String> {
    let existing_item_ids = state
        .store
        .find_by_document_id(doc.id())
        .await
        .map_err(|e| format!("failed to load existing document items: {}", e))?
        .into_iter()
        .map(|item| item.id().clone())
        .collect::<Vec<_>>();

    let new_item_ids = items
        .iter()
        .map(|item| item.id().clone())
        .collect::<Vec<_>>();
    let mut indexed_ids = Vec::with_capacity(items.len());

    for item in items {
        if let Err(err) = state.engine.index_item(item).await {
            let cleanup_error = cleanup_indexed_items(state, &indexed_ids).await;
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

    if let Err(err) = state.doc_store.save_with_replaced_items(doc, items).await {
        let cleanup_error = cleanup_indexed_items(state, &indexed_ids).await;
        return Err(with_cleanup_error(
            format!("failed to save document items transaction: {}", err),
            cleanup_error,
        ));
    }

    remove_obsolete_indexes(state, &existing_item_ids, &new_item_ids).await;

    Ok(new_item_ids.iter().map(ToString::to_string).collect())
}

async fn canonicalize_document(
    doc_store: &Arc<dyn DocumentRepository>,
    doc: Document,
) -> Result<Document, String> {
    let existing = doc_store
        .find_by_url(doc.url())
        .await
        .map_err(|e| format!("failed to find document by url before save: {}", e))?;
    let Some(existing) = existing else {
        return Ok(doc);
    };

    Ok(Document::restore(RestoreDocumentParams {
        id: existing.id().clone(),
        title: doc
            .title()
            .map(ToString::to_string)
            .or_else(|| existing.title().map(ToString::to_string)),
        raw_content: doc.raw_content().to_string(),
        source: doc.source().to_string(),
        url: doc.url().to_string(),
        source_version: doc.source_version().map(ToOwned::to_owned),
        captured_at: doc.captured_at(),
        created_at: existing.created_at(),
        updated_at: doc.updated_at(),
    }))
}

async fn cleanup_indexed_items(state: &Arc<AppState>, indexed_ids: &[ItemId]) -> Option<String> {
    let mut errors = Vec::new();

    for item_id in indexed_ids {
        if let Err(err) = state.engine.remove_from_index(item_id.as_str()).await {
            errors.push(format!("remove index {}: {}", item_id, err));
        }
    }

    if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    }
}

async fn remove_obsolete_indexes(
    state: &Arc<AppState>,
    old_item_ids: &[ItemId],
    new_item_ids: &[ItemId],
) {
    let new_item_ids = new_item_ids
        .iter()
        .map(|item_id| item_id.as_str())
        .collect::<HashSet<_>>();
    for item_id in old_item_ids {
        if new_item_ids.contains(item_id.as_str()) {
            continue;
        }
        if let Err(err) = state.engine.remove_from_index(item_id.as_str()).await {
            tracing::warn!(
                item_id = item_id.as_str(),
                error = %err,
                "failed to remove obsolete semantic index entry"
            );
        }
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

async fn set_job_running(state: &Arc<AppState>, job_id: &str) -> Result<(), String> {
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
    .await
}

async fn set_job_succeeded(state: &Arc<AppState>, job_id: &str) -> Result<(), String> {
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
    .await
}

async fn set_job_failed(state: &Arc<AppState>, job_id: &str, error: &str) -> Result<(), String> {
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
    .await
}

async fn set_conversation_status(
    state: &Arc<AppState>,
    conversation_id: &str,
    status: ConversationStatus,
) -> Result<(), String> {
    update_conversation(
        state,
        conversation_id,
        move |conversation| {
            conversation.status = status;
        },
        "status",
    )
    .await
}

async fn set_conversation_processed(
    state: &Arc<AppState>,
    conversation_id: &str,
    item_ids: Vec<String>,
) -> Result<(), String> {
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
    .await
}

async fn set_conversation_failed(
    state: &Arc<AppState>,
    conversation_id: &str,
    error: &str,
) -> Result<(), String> {
    update_conversation(
        state,
        conversation_id,
        |conversation| {
            conversation.status = ConversationStatus::Failed;
            conversation.last_error = Some(error.to_string());
        },
        "failed",
    )
    .await
}

async fn update_job<F>(
    state: &Arc<AppState>,
    job_id: &str,
    mutate: F,
    phase: &str,
) -> Result<(), String>
where
    F: FnOnce(&mut ExtractionJobRecord),
{
    let mut job = state
        .job_repo
        .find_job_by_id(job_id)
        .await
        .map_err(|err| format!("persist {} job skipped: load error {}", phase, err))?
        .ok_or_else(|| format!("persist {} job skipped: not found {}", phase, job_id))?;
    mutate(&mut job);
    state
        .job_repo
        .upsert_job(&job)
        .await
        .map_err(|err| format!("persist {} job failed: {}", phase, err))
}

async fn update_conversation<F>(
    state: &Arc<AppState>,
    conversation_id: &str,
    mutate: F,
    phase: &str,
) -> Result<(), String>
where
    F: FnOnce(&mut ConversationRecord),
{
    let mut conversation = state
        .conversation_repo
        .find_conversation_by_id(conversation_id)
        .await
        .map_err(|err| format!("persist {} conversation skipped: load error {}", phase, err))?
        .ok_or_else(|| {
            format!(
                "persist {} conversation skipped: not found {}",
                phase, conversation_id
            )
        })?;
    mutate(&mut conversation);
    state
        .conversation_repo
        .upsert_conversation(&conversation)
        .await
        .map_err(|err| format!("persist {} conversation failed: {}", phase, err))
}

#[cfg(test)]
mod tests {
    use super::{run_extraction, spawn_extraction};
    use crate::models::{
        now_iso, ConversationRecord, ConversationStatus, ExtractionJobRecord, ExtractionMode,
        JobStatus,
    };
    use crate::state::AppState;
    use async_trait::async_trait;
    use refine_core::conversation::{ConversationRepository, EventRepository, JobRepository};
    use refine_core::error::{InfraError, InfraResult};
    use refine_core::infra::{LlmClient, SqliteStore};
    use refine_core::knowledge::{Document, DocumentRepository, Item, ItemRepository};
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
            .await
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

    #[tokio::test]
    async fn semantic_index_failure_preserves_existing_document_items() {
        let (_tmp, state, persistence, conversation_id, job_id) = build_state_with_failing_index()
            .await
            .expect("build test state");
        let conversation = persistence
            .find_conversation_by_id(&conversation_id)
            .await
            .expect("load conversation")
            .expect("conversation exists");
        let mut doc = Document::new(&conversation.source, "previous raw");
        doc.set_url(&conversation.url);
        if let Some(title) = &conversation.title {
            doc.set_title(title);
        }
        let mut existing_item = Item::new_knowledge("existing", "old summary");
        existing_item.set_document_id(doc.id().clone());
        state
            .doc_store
            .save_with_replaced_items(&doc, &[existing_item.clone()])
            .await
            .expect("seed existing document items");

        let err = run_extraction(
            state.clone(),
            &conversation_id,
            &job_id,
            ExtractionMode::Auto,
        )
        .await
        .expect_err("index failure should fail extraction");
        assert!(
            err.contains("failed to index item"),
            "unexpected error: {}",
            err
        );

        let loaded_existing = state
            .store
            .find_by_id(existing_item.id())
            .await
            .expect("find existing item");
        assert!(loaded_existing.is_some());
        let linked = state
            .store
            .find_by_document_id(doc.id())
            .await
            .expect("find linked items");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id(), existing_item.id());
    }

    #[tokio::test]
    async fn invalid_job_start_transition_aborts_before_extraction() {
        let (_tmp, state, persistence, conversation_id, job_id) = build_state_with_failing_index()
            .await
            .expect("build test state");
        let mut job = persistence
            .find_job_by_id(&job_id)
            .await
            .expect("load job")
            .expect("job exists");
        job.status = JobStatus::Running;
        persistence
            .upsert_job(&job)
            .await
            .expect("mark job running");
        job.status = JobStatus::Succeeded;
        persistence
            .upsert_job(&job)
            .await
            .expect("mark job succeeded");

        let err = run_extraction(
            state.clone(),
            &conversation_id,
            &job_id,
            ExtractionMode::Auto,
        )
        .await
        .expect_err("succeeded job must not restart extraction");
        assert!(
            err.contains("invalid job status transition"),
            "unexpected error: {}",
            err
        );

        let saved_items = state.store.find_all().await.expect("load saved items");
        assert!(
            saved_items.is_empty(),
            "aborted extraction saved items: {:?}",
            saved_items
        );
    }

    async fn build_state_with_failing_index(
    ) -> Result<(TempDir, Arc<AppState>, Arc<SqliteStore>, String, String), String> {
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let db_path = tmp.path().join("refine-server.sqlite");
        let sqlite_store = Arc::new(SqliteStore::open(&db_path).map_err(|e| e.to_string())?);
        let store: Arc<dyn ItemRepository> = sqlite_store.clone();
        let doc_store: Arc<dyn DocumentRepository> = sqlite_store.clone();
        let conversation_repo: Arc<dyn ConversationRepository> = sqlite_store.clone();
        let job_repo: Arc<dyn JobRepository> = sqlite_store.clone();
        let event_repo: Arc<dyn EventRepository> = sqlite_store.clone();
        let engine = Arc::new(
            SearchEngine::new(store.clone()).with_vector_search(Arc::new(FailingVectorSearch)),
        );

        let conversation_id = Uuid::new_v4().to_string();
        let job_id = Uuid::new_v4().to_string();
        let now = now_iso();
        sqlite_store
            .upsert_conversation(&ConversationRecord {
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
            })
            .await
            .map_err(|e| e.to_string())?;
        sqlite_store
            .upsert_job(&ExtractionJobRecord {
                id: job_id.clone(),
                conversation_id: conversation_id.clone(),
                mode: ExtractionMode::Auto,
                status: JobStatus::Pending,
                created_at: now.clone(),
                updated_at: now,
                error: None,
            })
            .await
            .map_err(|e| e.to_string())?;

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
            conversation_repo,
            job_repo,
            event_repo,
        });

        Ok((tmp, state, sqlite_store, conversation_id, job_id))
    }

    async fn wait_for_failed_job(persistence: &SqliteStore, job_id: &str) -> ExtractionJobRecord {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let job = persistence
                .find_job_by_id(job_id)
                .await
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
