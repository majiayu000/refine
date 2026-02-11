use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::application::conversation::{
    create_conversation as run_create_conversation,
    create_extraction_job as run_create_extraction_job, CreateConversationError,
};
use crate::application::event::{
    create_event as run_create_event, get_event_summary as run_get_event_summary,
};
use crate::application::item::delete_item as run_delete_item;
use crate::application::job::get_extraction_job as run_get_extraction_job;
use crate::application::query::{
    get_quota as run_get_quota, list_conversations as run_list_conversations,
    list_items as run_list_items, search_items as run_search_items,
};
use crate::application::recommendation::recommend_items as run_recommend_items;
use crate::auth::authorize_user;
use crate::models::{
    CreateConversationRequest, CreateConversationResponse, CreateEventRequest, CreateEventResponse,
    CreateExtractionJobRequest, CreateExtractionJobResponse, EventSummaryQuery,
    GetExtractionJobResponse, ListConversationsQuery, ListItemsQuery, RecommendationQuery,
    SearchQuery,
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

    let response = CreateConversationResponse {
        conversation_id: result.conversation_id,
        status: result.status,
        deduplicated: if result.deduplicated { Some(true) } else { None },
        job_id: result.job_id,
    };
    match serde_json::to_value(response) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
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
        Ok(result) => match serde_json::to_value(CreateExtractionJobResponse {
            job_id: result.job_id,
            status: result.status,
        }) {
            Ok(payload) => ok(payload),
            Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
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

    match run_get_extraction_job(state, job_id).await {
        Ok(result) => match serde_json::to_value(GetExtractionJobResponse { job: result.job }) {
            Ok(payload) => ok(payload),
            Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
        Err(err) => err_response(err.status_code(), err.message()),
    }
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

    match run_create_event(state, user_id, payload) {
        Ok(result) => match serde_json::to_value(CreateEventResponse {
            event_id: result.event_id,
        }) {
            Ok(payload) => ok(payload),
            Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        },
        Err(err) => err_response(err.status_code(), err.message()),
    }
}

pub async fn get_event_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EventSummaryQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_get_event_summary(state, query.days) {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListConversationsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = run_list_conversations(state, query).await;
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub async fn list_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListItemsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_list_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub async fn get_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_get_quota(state).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub async fn delete_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_delete_item(state, item_id).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub async fn search_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let result = match run_search_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    match serde_json::to_value(result) {
        Ok(payload) => ok(payload),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
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

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
