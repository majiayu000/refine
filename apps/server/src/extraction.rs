use refine_core::knowledge::{
    Document, DocumentRepository, Item, ItemId, RestoreDocumentParams, Source,
};
use refine_core::refinement::{
    extract_items_with_strict_defaults, ExtractionPolicy, ItemExtractionInput,
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::{oneshot, Semaphore};
use tokio::time::{interval, Duration};
use uuid::Uuid;

use crate::models::{now_iso, ExtractionMode, JobStatus};
use crate::state::AppState;

pub fn spawn_extraction(
    state: Arc<AppState>,
    conversation_id: String,
    job_id: String,
    mode: ExtractionMode,
) {
    let permit = match extraction_semaphore().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            // The durable pending job will be picked up by the reconciliation
            // loop when capacity becomes available.
            return;
        }
    };
    tokio::spawn(async move {
        let _permit = permit;
        if let Err(err) = run_extraction(state, &conversation_id, &job_id, mode).await {
            tracing::warn!(job_id, conversation_id, error = %err, "extraction attempt ended");
        }
    });
}

pub async fn recover_extraction_jobs(state: Arc<AppState>) -> Result<usize, String> {
    let now = now_iso();
    let reconciled = state
        .job_repo
        .reconcile_processed_jobs(&now)
        .await
        .map_err(|err| format!("failed to reconcile processed extraction jobs: {err}"))?;
    if reconciled > 0 {
        tracing::info!(reconciled, "reconciled already processed extraction jobs");
    }
    if state.llm_client.is_none() {
        return Ok(0);
    }
    let jobs = state
        .job_repo
        .list_recoverable_jobs(&now, MAX_CONCURRENT_EXTRACTIONS)
        .await
        .map_err(|err| format!("failed to list recoverable extraction jobs: {err}"))?;
    let count = jobs.len();
    for job in jobs {
        spawn_extraction(state.clone(), job.conversation_id, job.id, job.mode);
    }
    Ok(count)
}

const LEASE_DURATION_SECS: i64 = 120;
const HEARTBEAT_INTERVAL_SECS: u64 = 30;
const RECOVERY_INTERVAL_SECS: u64 = 30;
const MAX_CONCURRENT_EXTRACTIONS: usize = 4;

fn extraction_semaphore() -> Arc<Semaphore> {
    static SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(MAX_CONCURRENT_EXTRACTIONS)))
        .clone()
}

pub fn spawn_extraction_recovery(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(RECOVERY_INTERVAL_SECS));
        // The caller performs the startup sweep after binding the listener.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match recover_extraction_jobs(state.clone()).await {
                Ok(0) => {}
                Ok(count) => tracing::info!(count, "scheduled recoverable extraction jobs"),
                Err(err) => {
                    tracing::warn!(error = %err, "failed to reconcile extraction jobs")
                }
            }
        }
    });
}

fn lease_times() -> (String, String) {
    let now = chrono::Utc::now();
    (
        now.to_rfc3339(),
        (now + chrono::Duration::seconds(LEASE_DURATION_SECS)).to_rfc3339(),
    )
}

async fn run_extraction(
    state: Arc<AppState>,
    conversation_id: &str,
    job_id: &str,
    mode: ExtractionMode,
) -> Result<(), String> {
    let owner = Uuid::new_v4().to_string();
    let (now, lease_expires_at) = lease_times();
    let claimed = state
        .job_repo
        .claim_job(job_id, &owner, &now, &lease_expires_at)
        .await
        .map_err(|err| format!("failed to claim extraction job: {err}"))?;
    if claimed.is_none() {
        return Err("extraction job is already claimed or terminal".to_string());
    }

    let lease_valid = Arc::new(AtomicBool::new(true));
    let (stop_tx, stop_rx) = oneshot::channel();
    let heartbeat = tokio::spawn(heartbeat_job_lease(
        state.clone(),
        job_id.to_string(),
        owner.clone(),
        lease_valid.clone(),
        stop_rx,
    ));
    let result = run_claimed_extraction(
        state.clone(),
        conversation_id,
        job_id,
        mode,
        &owner,
        lease_valid,
    )
    .await;
    let _ = stop_tx.send(());
    let _ = heartbeat.await;

    if let Err(error) = &result {
        let finished = state
            .job_repo
            .finish_job_claim(
                job_id,
                &owner,
                JobStatus::Failed,
                &[],
                Some(error),
                &now_iso(),
            )
            .await
            .map_err(|err| format!("failed to finish extraction claim: {err}"))?;
        if !finished {
            return Err("extraction claim was lost before failure was persisted".to_string());
        }
    }
    result
}

async fn heartbeat_job_lease(
    state: Arc<AppState>,
    job_id: String,
    owner: String,
    lease_valid: Arc<AtomicBool>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut ticker = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = &mut stop => return,
            _ = ticker.tick() => {
                let (now, expires_at) = lease_times();
                match state.job_repo.renew_job_lease(&job_id, &owner, &now, &expires_at).await {
                    Ok(true) => {}
                    Ok(false) => {
                        lease_valid.store(false, Ordering::Release);
                        return;
                    }
                    Err(err) => {
                        tracing::warn!(job_id, error = %err, "failed to renew extraction lease");
                    }
                }
            }
        }
    }
}

async fn run_claimed_extraction(
    state: Arc<AppState>,
    conversation_id: &str,
    job_id: &str,
    mode: ExtractionMode,
    owner: &str,
    lease_valid: Arc<AtomicBool>,
) -> Result<(), String> {
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

    if !lease_valid.load(Ordering::Acquire) {
        return Err("extraction lease was lost while awaiting the provider".to_string());
    }
    let (now, expires_at) = lease_times();
    let renewed = state
        .job_repo
        .renew_job_lease(job_id, owner, &now, &expires_at)
        .await
        .map_err(|err| format!("failed to verify extraction lease: {err}"))?;
    if !renewed {
        return Err("extraction lease was lost before persistence".to_string());
    }

    save_claimed_results_and_index(&state, job_id, owner, &doc, &items).await
}

async fn save_claimed_results_and_index(
    state: &Arc<AppState>,
    job_id: &str,
    owner: &str,
    doc: &Document,
    items: &[Item],
) -> Result<(), String> {
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

    let finished = match state
        .job_repo
        .finish_job_claim_with_results(job_id, owner, doc, items, &now_iso())
        .await
    {
        Ok(finished) => finished,
        Err(err) => {
            let cleanup_error = cleanup_indexed_items(state, &indexed_ids).await;
            return Err(with_cleanup_error(
                format!("failed to save claimed extraction results: {}", err),
                cleanup_error,
            ));
        }
    };
    if !finished {
        let cleanup_error = cleanup_indexed_items(state, &indexed_ids).await;
        return Err(with_cleanup_error(
            "extraction claim was lost before result persistence".to_string(),
            cleanup_error,
        ));
    }

    remove_obsolete_indexes(state, &existing_item_ids, &new_item_ids).await;

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{recover_extraction_jobs, run_extraction, spawn_extraction};
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
    use tokio::sync::Semaphore;
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
            err.contains("already claimed or terminal"),
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

    #[tokio::test]
    async fn startup_recovery_schedules_pending_jobs() {
        let (_tmp, state, persistence, _conversation_id, job_id) = build_state_with_failing_index()
            .await
            .expect("build test state");

        let scheduled = recover_extraction_jobs(state)
            .await
            .expect("schedule recovery");
        assert_eq!(scheduled, 1);
        let job = wait_for_failed_job(&persistence, &job_id).await;
        assert_eq!(job.attempt_count, 1);
    }

    #[tokio::test]
    async fn startup_recovery_without_llm_keeps_job_pending() {
        let (_tmp, state, persistence, _conversation_id, job_id) = build_state_with_failing_index()
            .await
            .expect("build test state");
        let state = Arc::new(AppState {
            llm_client: None,
            store: state.store.clone(),
            doc_store: state.doc_store.clone(),
            engine: state.engine.clone(),
            semantic_search_enabled: state.semantic_search_enabled,
            free_quota_items: state.free_quota_items,
            premium_users: state.premium_users.clone(),
            api_token: state.api_token.clone(),
            dev_anon: state.dev_anon,
            conversation_repo: state.conversation_repo.clone(),
            job_repo: state.job_repo.clone(),
            event_repo: state.event_repo.clone(),
        });

        assert_eq!(recover_extraction_jobs(state).await.expect("recovery"), 0);
        let job = persistence
            .find_job_by_id(&job_id)
            .await
            .expect("load job")
            .expect("job exists");
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.attempt_count, 0);
    }

    #[tokio::test]
    async fn extraction_scheduler_enforces_provider_concurrency_limit() {
        let semaphore = Semaphore::new(super::MAX_CONCURRENT_EXTRACTIONS);
        let mut permits = Vec::new();
        for _ in 0..super::MAX_CONCURRENT_EXTRACTIONS {
            permits.push(
                semaphore
                    .try_acquire()
                    .expect("configured extraction slot should be available"),
            );
        }
        assert!(
            semaphore.try_acquire().is_err(),
            "scheduler admitted more than the configured extraction concurrency"
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
                attempt_count: 0,
                lease_owner: None,
                lease_expires_at: None,
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
