use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::Json;
use refine_core::search::SearchQuery as CoreSearchQuery;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::authorize_user;
use crate::extraction::spawn_extraction;
use crate::models::{
    normalize_timestamp, now_iso, ConversationDto, ConversationRecord, ConversationStatus,
    CreateConversationRequest, CreateExtractionJobRequest, ExtractionJobRecord, ExtractionMode,
    ItemDto, JobStatus, ListConversationsQuery, ListItemsQuery, SearchQuery,
};
use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    ok(json!({
        "message": "Refine cloud API (Rust) is running"
    }))
}

pub async fn dashboard_page() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn create_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateConversationRequest>,
) -> impl IntoResponse {
    let user_id = match authorize_user(&headers, state.api_token.as_deref()) {
        Ok(user_id) => user_id,
        Err(err) => return err_response(StatusCode::UNAUTHORIZED, &err),
    };

    let content = match payload.content.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: content"),
    };
    let url = match payload.url.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: url"),
    };
    let source = match payload.source.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: source"),
    };
    let idempotency_key = match payload.idempotency_key.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => {
            return err_response(
                StatusCode::BAD_REQUEST,
                "Missing required field: idempotency_key",
            )
        }
    };

    if let Some(conversation_id) = find_conversation_by_idempotency(&state, &idempotency_key).await
    {
        let conversations = state.conversations.read().await;
        if let Some(record) = conversations.get(&conversation_id) {
            return ok(json!({
                "conversation_id": record.id,
                "status": record.status,
                "deduplicated": true
            }));
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

    let conversation = ConversationRecord {
        id: conversation_id.clone(),
        user_id,
        source,
        url,
        title: payload.title.filter(|v| !v.trim().is_empty()),
        raw_content: content,
        metadata: payload.metadata.unwrap_or_else(|| json!({})),
        captured_at: normalize_timestamp(payload.captured_at),
        created_at: now.clone(),
        status: conversation_status.clone(),
        idempotency_key: idempotency_key.clone(),
        item_ids: Vec::new(),
        last_error: None,
    };

    if let Err(err) = state.persistence.upsert_conversation(&conversation) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err);
    }

    {
        let mut conversations = state.conversations.write().await;
        conversations.insert(conversation_id.clone(), conversation);
    }
    {
        let mut idempotency = state.idempotency.write().await;
        idempotency.insert(idempotency_key, conversation_id.clone());
    }
    let mut job_id: Option<String> = None;
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
        if let Err(err) = state.persistence.upsert_job(&job) {
            return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err);
        }
        {
            let mut jobs = state.jobs.write().await;
            jobs.insert(id.clone(), job);
        }
        spawn_extraction(state, conversation_id.clone(), id.clone(), mode);
        job_id = Some(id);
    }
    let mut response = serde_json::Map::new();
    response.insert("conversation_id".to_string(), json!(conversation_id));
    response.insert("status".to_string(), json!(conversation_status));
    if let Some(id) = job_id {
        response.insert("job_id".to_string(), json!(id));
    }
    ok(serde_json::Value::Object(response))
}

pub async fn create_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateExtractionJobRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let conversation_id = match payload.conversation_id.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "conversation_id is required"),
    };

    let queued_conversation = {
        let mut conversations = state.conversations.write().await;
        let Some(conversation) = conversations.get_mut(&conversation_id) else {
            return err_response(StatusCode::NOT_FOUND, "Conversation not found");
        };
        conversation.status = ConversationStatus::Queued;
        conversation.last_error = None;
        conversation.clone()
    };
    if let Err(err) = state.persistence.upsert_conversation(&queued_conversation) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err);
    }

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
    if let Err(err) = state.persistence.upsert_job(&job) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err);
    }

    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    spawn_extraction(state, conversation_id, job_id.clone(), mode);

    ok(json!({
        "job_id": job_id,
        "status": JobStatus::Pending
    }))
}

pub async fn get_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let jobs = state.jobs.read().await;
    let Some(job) = jobs.get(&job_id).cloned() else {
        return err_response(StatusCode::NOT_FOUND, "Job not found");
    };

    ok(json!({ "job": job }))
}

pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListConversationsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let status_filter = query
        .status
        .map(|status| status.trim().to_ascii_lowercase())
        .filter(|status| !status.is_empty());

    let mut conversations: Vec<ConversationRecord> = {
        let guard = state.conversations.read().await;
        guard.values().cloned().collect()
    };

    if let Some(filter_status) = status_filter {
        conversations.retain(|record| conversation_status_name(&record.status) == filter_status);
    }

    conversations.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));

    let total = conversations.len();
    let paginated: Vec<ConversationDto> = conversations
        .into_iter()
        .skip(cursor)
        .take(limit)
        .map(|record| ConversationDto::from(&record))
        .collect();

    let next_cursor = if cursor + paginated.len() < total {
        Some(cursor + paginated.len())
    } else {
        None
    };

    ok(json!({
        "conversations": paginated,
        "total": total,
        "next_cursor": next_cursor
    }))
}

pub async fn list_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListItemsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let total = match state.store.count_items(None).await {
        Ok(total) => total,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let items = match state.store.find_recent(None, cursor, limit).await {
        Ok(items) => items,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let next_cursor = if cursor + items.len() < total {
        Some(cursor + items.len())
    } else {
        None
    };

    let data = items.iter().map(ItemDto::from).collect::<Vec<_>>();
    ok(json!({
        "items": data,
        "total": total,
        "next_cursor": next_cursor
    }))
}

pub async fn search_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let keyword = query.q.unwrap_or_default().trim().to_string();
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    if keyword.is_empty() {
        return ok(json!({ "items": [] }));
    }

    let result = match state
        .engine
        .search(CoreSearchQuery::new(&keyword).with_limit(limit))
        .await
    {
        Ok(result) => result,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let data = result
        .items
        .iter()
        .map(|hit| ItemDto::from(&hit.item))
        .collect::<Vec<_>>();

    ok(json!({ "items": data }))
}

async fn find_conversation_by_idempotency(state: &Arc<AppState>, key: &str) -> Option<String> {
    let index = state.idempotency.read().await;
    index.get(key).cloned()
}

fn ok(payload: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = serde_json::Map::new();
    body.insert("success".to_string(), serde_json::Value::Bool(true));
    if let serde_json::Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    (StatusCode::OK, Json(serde_json::Value::Object(body)))
}

fn err_response(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "success": false,
            "message": message
        })),
    )
}

fn conversation_status_name(status: &ConversationStatus) -> &'static str {
    match status {
        ConversationStatus::Captured => "captured",
        ConversationStatus::Queued => "queued",
        ConversationStatus::Processing => "processing",
        ConversationStatus::Processed => "processed",
        ConversationStatus::Failed => "failed",
    }
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
