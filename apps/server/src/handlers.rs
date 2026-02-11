use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;

use crate::application::conversation::{
    create_conversation as run_create_conversation,
    create_extraction_job as run_create_extraction_job, CreateConversationError,
};
use crate::application::error::ApplicationErrorCode;
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
use crate::models::{
    CreateConversationRequest, CreateConversationResponse, CreateEventRequest, CreateEventResponse,
    CreateExtractionJobRequest, CreateExtractionJobResponse, EventSummaryQuery,
    GetExtractionJobResponse, ListConversationsQuery, ListItemsQuery, RecommendationQuery,
    SearchQuery,
};
use crate::request_guard::{with_contract_header, AuthenticatedUser, SERVER_CONTRACT_VERSION};
use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    ok(json!({
        "message": "Refine cloud API (Rust) is running",
        "contract_version": SERVER_CONTRACT_VERSION
    }))
}

pub async fn dashboard_page() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn create_conversation(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(user_id): AuthenticatedUser,
    Json(payload): Json<CreateConversationRequest>,
) -> impl IntoResponse {
    let result = match run_create_conversation(state, user_id, payload).await {
        Ok(result) => result,
        Err(CreateConversationError::QuotaExceeded { used, limit }) => {
            return err_response_payload(
                StatusCode::FORBIDDEN,
                json!({
                    "message": format!("Free quota exceeded ({}/{} items). Upgrade required.", used, limit),
                    "quota": {
                        "used": used,
                        "limit": limit,
                        "remaining": 0,
                        "exceeded": true
                    }
                }),
            );
        }
        Err(err) => return err_response(status_from_error_code(err.code()), &err.message()),
    };

    let response = CreateConversationResponse {
        conversation_id: result.conversation_id,
        status: result.status,
        deduplicated: if result.deduplicated {
            Some(true)
        } else {
            None
        },
        job_id: result.job_id,
    };
    ok_serializable(response)
}

pub async fn create_extraction_job(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Json(payload): Json<CreateExtractionJobRequest>,
) -> impl IntoResponse {
    match run_create_extraction_job(state, payload).await {
        Ok(result) => ok_serializable(CreateExtractionJobResponse {
            job_id: result.job_id,
            status: result.status,
        }),
        Err(err) => err_response(status_from_error_code(err.code()), err.message()),
    }
}

pub async fn get_extraction_job(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    match run_get_extraction_job(state, job_id).await {
        Ok(result) => ok_serializable(GetExtractionJobResponse { job: result.job }),
        Err(err) => err_response(status_from_error_code(err.code()), err.message()),
    }
}

pub async fn create_event(
    State(state): State<Arc<AppState>>,
    AuthenticatedUser(user_id): AuthenticatedUser,
    Json(payload): Json<CreateEventRequest>,
) -> impl IntoResponse {
    match run_create_event(state, user_id, payload) {
        Ok(result) => ok_serializable(CreateEventResponse {
            event_id: result.event_id,
        }),
        Err(err) => err_response(status_from_error_code(err.code()), err.message()),
    }
}

pub async fn get_event_summary(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Query(query): Query<EventSummaryQuery>,
) -> impl IntoResponse {
    let result = match run_get_event_summary(state, query.days) {
        Ok(result) => result,
        Err(err) => return err_response(status_from_error_code(err.code()), err.message()),
    };
    ok_serializable(result)
}

pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Query(query): Query<ListConversationsQuery>,
) -> impl IntoResponse {
    let result = run_list_conversations(state, query).await;
    ok_serializable(result)
}

pub async fn list_items(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Query(query): Query<ListItemsQuery>,
) -> impl IntoResponse {
    let result = match run_list_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(status_from_error_code(err.code()), err.message()),
    };
    ok_serializable(result)
}

pub async fn get_quota(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
) -> impl IntoResponse {
    let result = match run_get_quota(state).await {
        Ok(result) => result,
        Err(err) => return err_response(status_from_error_code(err.code()), err.message()),
    };
    ok_serializable(result)
}

pub async fn delete_item(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Path(item_id): Path<String>,
) -> impl IntoResponse {
    let result = match run_delete_item(state, item_id).await {
        Ok(result) => result,
        Err(err) => return err_response(status_from_error_code(err.code()), err.message()),
    };
    ok_serializable(result)
}

pub async fn search_items(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    let result = match run_search_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(status_from_error_code(err.code()), err.message()),
    };
    ok_serializable(result)
}

pub async fn recommend_items(
    State(state): State<Arc<AppState>>,
    _auth: AuthenticatedUser,
    Query(query): Query<RecommendationQuery>,
) -> impl IntoResponse {
    let result = match run_recommend_items(state, query.q, query.limit).await {
        Ok(result) => result,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err),
    };
    ok_serializable(result)
}

fn ok(payload: serde_json::Value) -> Response {
    let mut body = serde_json::Map::new();
    body.insert("success".to_string(), serde_json::Value::Bool(true));
    if let serde_json::Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    with_contract_header((StatusCode::OK, Json(serde_json::Value::Object(body))).into_response())
}

fn err_response(status: StatusCode, message: &str) -> Response {
    err_response_payload(
        status,
        json!({
            "message": message
        }),
    )
}

fn err_response_payload(status: StatusCode, payload: serde_json::Value) -> Response {
    let mut body = serde_json::Map::new();
    body.insert("success".to_string(), serde_json::Value::Bool(false));
    if let serde_json::Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    with_contract_header((status, Json(serde_json::Value::Object(body))).into_response())
}

fn ok_serializable<T>(payload: T) -> Response
where
    T: Serialize,
{
    match serde_json::to_value(payload) {
        Ok(value) => ok(value),
        Err(err) => err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

fn status_from_error_code(code: ApplicationErrorCode) -> StatusCode {
    match code {
        ApplicationErrorCode::BadRequest => StatusCode::BAD_REQUEST,
        ApplicationErrorCode::Forbidden => StatusCode::FORBIDDEN,
        ApplicationErrorCode::NotFound => StatusCode::NOT_FOUND,
        ApplicationErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
