use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::Json;
use chrono::{Duration, Utc};
use refine_core::knowledge::ItemId;
use refine_core::search::SearchQuery as CoreSearchQuery;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::application::conversation::{
    create_conversation as run_create_conversation,
    create_extraction_job as run_create_extraction_job, CreateConversationError,
};
use crate::application::recommendation::recommend_items as run_recommend_items;
use crate::auth::authorize_user;
use crate::models::{
    normalize_timestamp, ConversationDto, ConversationRecord, ConversationStatus,
    CreateConversationRequest, CreateEventRequest, CreateExtractionJobRequest, EventRecord,
    EventSummaryQuery, ItemDto, ListConversationsQuery, ListItemsQuery, RecommendationQuery,
    SearchQuery,
};
use crate::state::AppState;

const FUNNEL_EVENTS: [&str; 5] = [
    "conversation_extracted",
    "conversation_synced",
    "recommendation_exposed",
    "recommendation_clicked",
    "knowledge_reused",
];

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

    let result = match run_create_conversation(state, user_id, payload).await {
        Ok(result) => result,
        Err(CreateConversationError::QuotaExceeded { used, limit }) => {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "success": false,
                    "message": format!("Free quota exceeded ({}/{} items). Upgrade required.", used, limit),
                    "quota": {
                        "used": used,
                        "limit": limit,
                        "remaining": 0,
                        "exceeded": true
                    }
                })),
            );
        }
        Err(err) => return err_response(err.status_code(), &err.message()),
    };

    let mut response = serde_json::Map::new();
    response.insert("conversation_id".to_string(), json!(result.conversation_id));
    response.insert("status".to_string(), json!(result.status));
    if result.deduplicated {
        response.insert("deduplicated".to_string(), json!(true));
    }
    if let Some(id) = result.job_id {
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

    match run_create_extraction_job(state, payload).await {
        Ok(result) => ok(json!({
            "job_id": result.job_id,
            "status": result.status
        })),
        Err(err) => err_response(err.status_code(), err.message()),
    }
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

pub async fn create_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateEventRequest>,
) -> impl IntoResponse {
    let user_id = match authorize_user(&headers, state.api_token.as_deref()) {
        Ok(user_id) => user_id,
        Err(err) => return err_response(StatusCode::UNAUTHORIZED, &err),
    };

    let event_name = match payload.event_name.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => value,
        _ => return err_response(StatusCode::BAD_REQUEST, "event_name is required"),
    };

    let source = payload
        .source
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let properties = normalize_event_properties(payload.properties);

    let event = EventRecord {
        id: Uuid::new_v4().to_string(),
        user_id,
        event_name,
        source,
        properties,
        created_at: normalize_timestamp(payload.occurred_at),
    };

    if let Err(err) = state.persistence.insert_event(&event) {
        return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err);
    }

    ok(json!({
        "event_id": event.id
    }))
}

pub async fn get_event_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EventSummaryQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let days = query.days.unwrap_or(7).clamp(1, 90);
    let since = (Utc::now() - Duration::days(days as i64)).to_rfc3339();

    let pairs = match state.persistence.event_counts_since(Some(&since)) {
        Ok(pairs) => pairs,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    };

    let mut counts = serde_json::Map::new();
    for event_name in FUNNEL_EVENTS {
        counts.insert(event_name.to_string(), json!(0));
    }
    for (event_name, count) in pairs {
        counts.insert(event_name, json!(count));
    }

    ok(json!({
        "days": days,
        "since": since,
        "counts": counts
    }))
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

pub async fn get_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let used = match state.store.count_items(None).await {
        Ok(total) => total,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    if state.free_quota_items == 0 {
        return ok(json!({
            "limit": null,
            "used": used,
            "remaining": null,
            "exceeded": false
        }));
    }

    let remaining = state.free_quota_items.saturating_sub(used);
    let exceeded = used >= state.free_quota_items;

    ok(json!({
        "limit": state.free_quota_items,
        "used": used,
        "remaining": remaining,
        "exceeded": exceeded
    }))
}

pub async fn delete_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let normalized_id = item_id.trim().to_string();
    if normalized_id.is_empty() {
        return err_response(StatusCode::BAD_REQUEST, "item_id is required");
    }

    let deleted = match state.store.delete(&ItemId::from_str(&normalized_id)).await {
        Ok(deleted) => deleted,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    if !deleted {
        return err_response(StatusCode::NOT_FOUND, "Item not found");
    }

    if let Err(err) = state.engine.remove_from_index(&normalized_id).await {
        tracing::warn!(
            "item {} removed from store but failed to remove from vector index: {}",
            normalized_id,
            err
        );
    }

    ok(json!({
        "deleted": true,
        "id": normalized_id
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

pub async fn recommend_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RecommendationQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_recommend_items(state, query.q, query.limit).await {
        Ok(result) => result,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
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

fn normalize_event_properties(raw: Option<serde_json::Value>) -> serde_json::Value {
    match raw {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        _ => json!({}),
    }
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
