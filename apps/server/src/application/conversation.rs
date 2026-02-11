use axum::http::StatusCode;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::extraction::spawn_extraction;
use crate::models::{
    normalize_timestamp, now_iso, ConversationStatus, CreateConversationRequest,
    CreateExtractionJobRequest, ExtractionJobRecord, ExtractionMode, JobStatus,
};
use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct CreateConversationResult {
    pub conversation_id: String,
    pub status: ConversationStatus,
    pub deduplicated: bool,
    pub job_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CreateConversationError {
    BadRequest(String),
    QuotaExceeded { used: usize, limit: usize },
    Internal(String),
}

impl CreateConversationError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::QuotaExceeded { .. } => StatusCode::FORBIDDEN,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::BadRequest(message) => message.clone(),
            Self::QuotaExceeded { used, limit } => {
                format!(
                    "Free quota exceeded ({}/{} items). Upgrade required.",
                    used, limit
                )
            }
            Self::Internal(message) => message.clone(),
        }
    }
}

pub async fn create_conversation(
    state: Arc<AppState>,
    user_id: String,
    payload: CreateConversationRequest,
) -> Result<CreateConversationResult, CreateConversationError> {
    let content =
        trim_required(payload.content, "content").map_err(CreateConversationError::BadRequest)?;
    let url = trim_required(payload.url, "url").map_err(CreateConversationError::BadRequest)?;
    let source =
        trim_required(payload.source, "source").map_err(CreateConversationError::BadRequest)?;
    let idempotency_key = trim_required(payload.idempotency_key, "idempotency_key")
        .map_err(CreateConversationError::BadRequest)?;

    if let Some(conversation_id) = find_conversation_by_idempotency(&state, &idempotency_key).await
    {
        let conversations = state.runtime.conversations.read().await;
        if let Some(record) = conversations.get(&conversation_id) {
            return Ok(CreateConversationResult {
                conversation_id: record.id.clone(),
                status: record.status.clone(),
                deduplicated: true,
                job_id: None,
            });
        }
    }

    if state.free_quota_items > 0 {
        let used = state
            .store
            .count_items(None)
            .await
            .map_err(|err| CreateConversationError::Internal(err.to_string()))?;
        if used >= state.free_quota_items {
            return Err(CreateConversationError::QuotaExceeded {
                used,
                limit: state.free_quota_items,
            });
        }
    }

    let now = now_iso();
    let conversation_id = Uuid::new_v4().to_string();
    let mode = ExtractionMode::Auto;
    let ingest_only = payload.ingest_only.unwrap_or(false);
    let conversation_status = if ingest_only {
        ConversationStatus::Captured
    } else {
        ConversationStatus::Queued
    };
    let title = payload
        .title
        .and_then(|value| trim_optional(value.as_str()).map(ToString::to_string));

    let conversation = crate::models::ConversationRecord {
        id: conversation_id.clone(),
        user_id,
        source,
        url,
        title,
        raw_content: content,
        metadata: payload.metadata.unwrap_or_else(|| json!({})),
        captured_at: normalize_timestamp(payload.captured_at),
        created_at: now.clone(),
        status: conversation_status.clone(),
        idempotency_key: idempotency_key.clone(),
        item_ids: Vec::new(),
        last_error: None,
    };

    state
        .conversation_repo
        .upsert_conversation(&conversation)
        .map_err(CreateConversationError::Internal)?;

    {
        let mut conversations = state.runtime.conversations.write().await;
        conversations.insert(conversation_id.clone(), conversation);
    }
    {
        let mut idempotency = state.runtime.idempotency.write().await;
        idempotency.insert(idempotency_key, conversation_id.clone());
    }

    let mut job_id = None;
    if !ingest_only {
        let id = Uuid::new_v4().to_string();
        let job = ExtractionJobRecord {
            id: id.clone(),
            conversation_id: conversation_id.clone(),
            mode: mode.clone(),
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
        };
        state
            .job_repo
            .upsert_job(&job)
            .map_err(CreateConversationError::Internal)?;
        {
            let mut jobs = state.runtime.jobs.write().await;
            jobs.insert(id.clone(), job);
        }
        spawn_extraction(state, conversation_id.clone(), id.clone(), mode);
        job_id = Some(id);
    }

    Ok(CreateConversationResult {
        conversation_id,
        status: conversation_status,
        deduplicated: false,
        job_id,
    })
}

#[derive(Debug, Clone)]
pub struct CreateExtractionJobResult {
    pub job_id: String,
    pub status: JobStatus,
}

#[derive(Debug, Clone)]
pub enum CreateExtractionJobError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl CreateExtractionJobError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(message) => message,
            Self::NotFound(message) => message,
            Self::Internal(message) => message,
        }
    }
}

pub async fn create_extraction_job(
    state: Arc<AppState>,
    payload: CreateExtractionJobRequest,
) -> Result<CreateExtractionJobResult, CreateExtractionJobError> {
    let conversation_id =
        trim_required(payload.conversation_id, "conversation_id").map_err(|_| {
            CreateExtractionJobError::BadRequest("conversation_id is required".to_string())
        })?;

    let queued_conversation = {
        let mut conversations = state.runtime.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&conversation_id) else {
            return Err(CreateExtractionJobError::NotFound(
                "Conversation not found".to_string(),
            ));
        };
        conversation.status = ConversationStatus::Queued;
        conversation.last_error = None;
        conversation.clone()
    };
    state
        .conversation_repo
        .upsert_conversation(&queued_conversation)
        .map_err(CreateExtractionJobError::Internal)?;

    let mode = ExtractionMode::from_option(payload.mode);
    let now = now_iso();
    let job_id = Uuid::new_v4().to_string();
    let job = ExtractionJobRecord {
        id: job_id.clone(),
        conversation_id: conversation_id.clone(),
        mode: mode.clone(),
        status: JobStatus::Pending,
        created_at: now.clone(),
        updated_at: now,
        error: None,
    };

    state
        .job_repo
        .upsert_job(&job)
        .map_err(CreateExtractionJobError::Internal)?;
    {
        let mut jobs = state.runtime.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    spawn_extraction(state, conversation_id, job_id.clone(), mode);

    Ok(CreateExtractionJobResult {
        job_id,
        status: JobStatus::Pending,
    })
}

fn trim_required(value: Option<String>, field_name: &str) -> Result<String, String> {
    value
        .and_then(|value| trim_optional(value.as_str()).map(ToString::to_string))
        .ok_or_else(|| format!("Missing required field: {}", field_name))
}

fn trim_optional(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

async fn find_conversation_by_idempotency(state: &Arc<AppState>, key: &str) -> Option<String> {
    let index = state.runtime.idempotency.read().await;
    index.get(key).cloned()
}

#[cfg(test)]
mod tests {
    use super::{trim_optional, trim_required, CreateConversationError, CreateExtractionJobError};
    use axum::http::StatusCode;

    #[test]
    fn trim_required_reports_missing_field_name() {
        let err = trim_required(None, "content").expect_err("missing field should fail");
        assert_eq!(err, "Missing required field: content");
    }

    #[test]
    fn create_conversation_error_keeps_original_bad_request_message() {
        let err = CreateConversationError::BadRequest("custom error".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(err.message(), "custom error");
    }

    #[test]
    fn trim_optional_rejects_blank_values() {
        assert_eq!(trim_optional("   "), None);
        assert_eq!(trim_optional(" value "), Some("value"));
    }

    #[test]
    fn extraction_job_error_maps_to_http_status() {
        let not_found = CreateExtractionJobError::NotFound("Conversation not found".to_string());
        assert_eq!(not_found.status_code(), StatusCode::NOT_FOUND);
        assert_eq!(not_found.message(), "Conversation not found");
    }
}
