use refine_core::knowledge::{Document, DocumentId, DocumentRepository, Source};
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

    let mut item_ids = Vec::with_capacity(items.len());
    for item in &items {
        state.store.save(item).await.map_err(|e| e.to_string())?;
        if let Err(err) = state.engine.index_item(item).await {
            tracing::warn!(
                "failed to index item {} for semantic search: {}",
                item.id(),
                err
            );
        }
        item_ids.push(item.id().to_string());
    }

    set_conversation_processed(&state, conversation_id, item_ids).await;
    set_job_succeeded(&state, job_id).await;

    Ok(())
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
    let mut job = match state.job_repo.find_job_by_id(job_id).await {
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
    if let Err(err) = state.job_repo.upsert_job(&job).await {
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
        .await
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
    if let Err(err) = state
        .conversation_repo
        .upsert_conversation(&conversation)
        .await
    {
        tracing::warn!("persist {} conversation failed: {}", phase, err);
    }
}
