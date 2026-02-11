use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde::Serialize;
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

const SERVER_CONTRACT_VERSION: &str = "1.0";
const CONTRACT_VERSION_HEADER: &str = "x-refine-contract-version";

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
    headers: HeaderMap,
    Json(payload): Json<CreateConversationRequest>,
) -> impl IntoResponse {
    let user_id = match authorize_with_state(&headers, &state) {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };

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
        Err(err) => return err_response(err.status_code(), &err.message()),
    };

    let response = CreateConversationResponse {
        conversation_id: result.conversation_id,
        status: result.status,
        deduplicated: if result.deduplicated { Some(true) } else { None },
        job_id: result.job_id,
    };
    ok_serializable(response)
}

pub async fn create_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateExtractionJobRequest>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    match run_create_extraction_job(state, payload).await {
        Ok(result) => ok_serializable(CreateExtractionJobResponse {
            job_id: result.job_id,
            status: result.status,
        }),
        Err(err) => err_response(err.status_code(), err.message()),
    }
}

pub async fn get_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    match run_get_extraction_job(state, job_id).await {
        Ok(result) => ok_serializable(GetExtractionJobResponse { job: result.job }),
        Err(err) => err_response(err.status_code(), err.message()),
    }
}

pub async fn create_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateEventRequest>,
) -> impl IntoResponse {
    let user_id = match authorize_with_state(&headers, &state) {
        Ok(user_id) => user_id,
        Err(response) => return response,
    };

    match run_create_event(state, user_id, payload) {
        Ok(result) => ok_serializable(CreateEventResponse {
            event_id: result.event_id,
        }),
        Err(err) => err_response(err.status_code(), err.message()),
    }
}

pub async fn get_event_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<EventSummaryQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = match run_get_event_summary(state, query.days) {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    ok_serializable(result)
}

pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListConversationsQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = run_list_conversations(state, query).await;
    ok_serializable(result)
}

pub async fn list_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListItemsQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = match run_list_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    ok_serializable(result)
}

pub async fn get_quota(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = match run_get_quota(state).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    ok_serializable(result)
}

pub async fn delete_item(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = match run_delete_item(state, item_id).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    ok_serializable(result)
}

pub async fn search_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

    let result = match run_search_items(state, query).await {
        Ok(result) => result,
        Err(err) => return err_response(err.status_code(), err.message()),
    };
    ok_serializable(result)
}

pub async fn recommend_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RecommendationQuery>,
) -> impl IntoResponse {
    if let Err(response) = authorize_required(&headers, &state) {
        return response;
    }

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

fn authorize_with_state(headers: &HeaderMap, state: &AppState) -> Result<String, Response> {
    validate_client_contract(headers)?;
    authorize_user(headers, state.api_token.as_deref())
        .map_err(|err| err_response(StatusCode::UNAUTHORIZED, &err))
}

fn authorize_required(headers: &HeaderMap, state: &AppState) -> Result<(), Response> {
    authorize_with_state(headers, state).map(|_| ())
}

fn validate_client_contract(headers: &HeaderMap) -> Result<(), Response> {
    let Some(raw) = headers.get(CONTRACT_VERSION_HEADER) else {
        return Ok(());
    };
    let raw = raw
        .to_str()
        .map_err(|_| err_response(StatusCode::BAD_REQUEST, "invalid contract version header"))?;
    if raw.trim().is_empty() {
        return Ok(());
    }
    if is_contract_compatible(raw) {
        return Ok(());
    }
    Err(err_response(
        StatusCode::UPGRADE_REQUIRED,
        &format!(
            "Client contract version {} is incompatible with server {}",
            raw, SERVER_CONTRACT_VERSION
        ),
    ))
}

fn is_contract_compatible(client_version: &str) -> bool {
    let client_major = normalize_contract_major(client_version);
    let server_major = normalize_contract_major(SERVER_CONTRACT_VERSION);
    !client_major.is_empty() && client_major == server_major
}

fn normalize_contract_major(version: &str) -> &str {
    let version = version.trim();
    version.split('.').next().unwrap_or(version)
}

fn with_contract_header(mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(SERVER_CONTRACT_VERSION) {
        response.headers_mut().insert(CONTRACT_VERSION_HEADER, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{
        is_contract_compatible, normalize_contract_major, validate_client_contract,
        CONTRACT_VERSION_HEADER,
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode};

    #[test]
    fn normalize_contract_major_uses_first_segment() {
        assert_eq!(normalize_contract_major("1.2.3"), "1");
        assert_eq!(normalize_contract_major(" 2 "), "2");
    }

    #[test]
    fn contract_compatibility_checks_major_version() {
        assert!(is_contract_compatible("1.0"));
        assert!(is_contract_compatible("1.9.9"));
        assert!(!is_contract_compatible("2.0"));
    }

    #[test]
    fn validate_contract_allows_missing_header() {
        let headers = HeaderMap::new();
        assert!(validate_client_contract(&headers).is_ok());
    }

    #[test]
    fn validate_contract_rejects_incompatible_version() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTRACT_VERSION_HEADER, HeaderValue::from_static("2.0"));
        let response = validate_client_contract(&headers).expect_err("expected mismatch error");
        assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
    }
}

const DASHBOARD_HTML: &str = include_str!("dashboard.html");
