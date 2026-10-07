use crate::{
    build_app,
    state::{AppState, AuthConfig},
};
use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::cors::AllowOrigin;

mod quota;

async fn fixture() -> (tempfile::TempDir, Arc<AppState>, Router) {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(
        AppState::build_for_test(
            dir.path().join("shared.sqlite"),
            AuthConfig {
                api_token: Some("test-token".into()),
                dev_anon: false,
            },
        )
        .await
        .unwrap(),
    );
    let app = build_app(state.clone(), AllowOrigin::list([]));
    (dir, state, app)
}

async fn post(app: &Router, payload: Value) -> Value {
    post_expect(app, payload, StatusCode::OK).await
}

async fn post_expect(app: &Router, payload: Value, status: StatusCode) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/conversations")
                .header(header::AUTHORIZATION, "Bearer test-token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), status);
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}

#[tokio::test]
async fn captured_receipt_replay_succeeds_after_quota_fills_without_admitting_new_payloads() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::build_for_test(
        dir.path().join("quota.sqlite"),
        AuthConfig {
            api_token: Some("test-token".into()),
            dev_anon: false,
        },
    )
    .await
    .unwrap();
    state.free_quota_items = 1;
    state.premium_users.clear();
    let state = Arc::new(state);
    let app = build_app(state.clone(), AllowOrigin::list([]));
    let payload = json!({
        "content": "A capture whose first response was lost", "source": "browser",
        "url": "https://example.test/lost-response", "idempotency_key": "lost-response",
        "ingest_only": true, "metadata": {"capture": 1}
    });
    let original = post(&app, payload.clone()).await;
    state
        .store
        .save(&refine_core::knowledge::Item::new_knowledge(
            "quota", "one item",
        ))
        .await
        .unwrap();
    let replay = post(&app, payload.clone()).await;
    assert_eq!(replay["conversation_id"], original["conversation_id"]);
    assert_eq!(replay["status"], "captured");
    assert_eq!(replay["deduplicated"], true);

    let mut conflict = payload.clone();
    conflict["content"] = json!("another payload must use another key");
    post_expect(&app, conflict, StatusCode::BAD_REQUEST).await;
    let mut new_request = payload;
    new_request["idempotency_key"] = json!("genuinely-new");
    let rejected = post_expect(&app, new_request, StatusCode::FORBIDDEN).await;
    assert_eq!(rejected["quota"]["used"], 1);
    assert_eq!(
        state
            .conversation_repo
            .count_conversations(None)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn shared_router_preserves_capture_receipt_metadata_and_idempotency_without_llm() {
    let (_dir, state, app) = fixture().await;
    assert!(state.llm_client.is_none());
    let payload = json!({
        "content": "User: discuss safe rollout\nAssistant: use a canary deployment",
        "source": "chatgpt", "url": "https://chatgpt.com/c/capture-only", "title": "Capture only",
        "idempotency_key": "capture-v1", "ingest_only": true,
        "metadata": { "capture": { "generation": 7 } }
    });
    let first = post(&app, payload.clone()).await;
    let second = post(&app, payload).await;
    assert_eq!(first["status"], "captured");
    assert!(first.get("job_id").is_none());
    assert_eq!(first["conversation_id"], second["conversation_id"]);
    assert_eq!(second["deduplicated"], true);
    let id = first["conversation_id"].as_str().unwrap();
    let persisted = state
        .conversation_repo
        .find_conversation_by_id(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.metadata,
        json!({ "capture": { "generation": 7 } })
    );
    assert_eq!(
        state
            .conversation_repo
            .count_conversations(None)
            .await
            .unwrap(),
        1
    );
    assert_eq!(state.store.count_items(None).await.unwrap(), 0);
    assert!(state
        .job_repo
        .list_recoverable_jobs(&crate::models::now_iso(), 100)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn shared_router_returns_a_real_queryable_extraction_job() {
    let (_dir, state, app) = fixture().await;
    let receipt = post(
        &app,
        json!({
            "content": "User: save this conversation for extraction", "source": "chatgpt",
            "url": "https://chatgpt.com/c/queued", "idempotency_key": "queued-v1"
        }),
    )
    .await;
    let id = receipt["conversation_id"].as_str().unwrap();
    let job_id = receipt["job_id"].as_str().unwrap();
    assert!(state
        .conversation_repo
        .find_conversation_by_id(id)
        .await
        .unwrap()
        .is_some());
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/v1/extraction-jobs/{job_id}"))
                .header(header::AUTHORIZATION, "Bearer test-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(body["job"]["id"], job_id);
    assert_eq!(body["job"]["conversation_id"], id);
}

#[tokio::test]
async fn native_unconfigured_api_does_not_claim_a_discovery_port() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(
        AppState::build_for_test(
            dir.path().join("native.sqlite"),
            AuthConfig {
                api_token: None,
                dev_anon: false,
            },
        )
        .await
        .unwrap(),
    );
    let error = crate::serve_embedded(state, 0).await.unwrap_err();
    assert!(error.contains("REFINE_API_TOKEN"));
}

#[tokio::test]
async fn discovery_requires_matching_database_access_and_runtime_configuration() {
    let (dir, state, app) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    assert!(crate::is_refine_server(&address, &state).await);
    let mut expected = AppState::build_for_test(
        dir.path().join("shared.sqlite"),
        AuthConfig {
            api_token: Some("different-token".into()),
            dev_anon: false,
        },
    )
    .await
    .unwrap();
    assert!(!crate::is_refine_server(&address, &expected).await);
    expected.api_token = state.api_token.clone();
    expected.semantic_search_enabled = true;
    assert!(!crate::is_refine_server(&address, &expected).await);
    expected.semantic_search_enabled = false;
    expected.database_identity = "another-database".into();
    assert!(!crate::is_refine_server(&address, &expected).await);
    server.abort();
}
