use refine_core::knowledge::Source;
use refine_core::refinement::{
    extract_items_with_defaults, ExtractionPolicy, ItemExtractionInput,
};
use std::sync::Arc;

use crate::models::{now_iso, ConversationStatus, ExtractionMode, JobStatus};
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

    let conversation = {
        let guard = state.runtime.conversations.read().await;
        guard
            .get(conversation_id)
            .cloned()
            .ok_or_else(|| "Conversation not found".to_string())?
    };

    let source = Source::new(&conversation.source)
        .with_conversation_id(&conversation.id)
        .with_url(&conversation.url);
    let input = ItemExtractionInput {
        source: &conversation.source,
        title: conversation.title.as_deref(),
        raw_content: &conversation.raw_content,
        captured_at: Some(&conversation.captured_at),
        policy: mode_to_policy(mode),
    };
    let items = extract_items_with_defaults(state.llm_client.as_deref(), &input, &source).await;

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
    let job_snapshot = {
        let mut jobs = state.runtime.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.updated_at = now_iso();
            job.error = None;
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.job_repo.upsert_job(&job) {
            tracing::warn!("persist running job failed: {}", err);
        }
    }
}

async fn set_job_succeeded(state: &Arc<AppState>, job_id: &str) {
    let job_snapshot = {
        let mut jobs = state.runtime.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Succeeded;
            job.updated_at = now_iso();
            job.error = None;
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.job_repo.upsert_job(&job) {
            tracing::warn!("persist succeeded job failed: {}", err);
        }
    }
}

async fn set_job_failed(state: &Arc<AppState>, job_id: &str, error: &str) {
    let job_snapshot = {
        let mut jobs = state.runtime.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.updated_at = now_iso();
            job.error = Some(error.to_string());
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.job_repo.upsert_job(&job) {
            tracing::warn!("persist failed job failed: {}", err);
        }
    }
}

async fn set_conversation_status(
    state: &Arc<AppState>,
    conversation_id: &str,
    status: ConversationStatus,
) {
    let conversation_snapshot = {
        let mut conversations = state.runtime.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = status;
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.conversation_repo.upsert_conversation(&conversation) {
            tracing::warn!("persist conversation status failed: {}", err);
        }
    }
}

async fn set_conversation_processed(
    state: &Arc<AppState>,
    conversation_id: &str,
    item_ids: Vec<String>,
) {
    let conversation_snapshot = {
        let mut conversations = state.runtime.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = ConversationStatus::Processed;
            conversation.item_ids = item_ids;
            conversation.last_error = None;
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.conversation_repo.upsert_conversation(&conversation) {
            tracing::warn!("persist processed conversation failed: {}", err);
        }
    }
}

async fn set_conversation_failed(state: &Arc<AppState>, conversation_id: &str, error: &str) {
    let conversation_snapshot = {
        let mut conversations = state.runtime.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = ConversationStatus::Failed;
            conversation.last_error = Some(error.to_string());
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.conversation_repo.upsert_conversation(&conversation) {
            tracing::warn!("persist failed conversation failed: {}", err);
        }
    }
}
