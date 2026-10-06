use refine_core::conversation::{
    ConversationRecord, ConversationStatus, ExtractionJobRecord, ExtractionMode, JobStatus,
};
use refine_core::knowledge::Item;

use super::*;

async fn quota_fixture() -> (tempfile::TempDir, Arc<AppState>, Router) {
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
    (dir, state, app)
}

fn payload(key: &str, ingest_only: bool) -> Value {
    json!({
        "content": "User: save a database decision",
        "source": "chatgpt", "url": "https://chatgpt.com/c/quota",
        "idempotency_key": key, "ingest_only": ingest_only,
    })
}

async fn fill_quota(state: &AppState) {
    state
        .store
        .save(&Item::new_knowledge(
            "Existing item",
            "Fills the configured quota",
        ))
        .await
        .unwrap();
    assert_eq!(state.store.count_items(None).await.unwrap(), 1);
}

#[tokio::test]
async fn lost_capture_receipt_can_be_replayed_after_quota_fills() {
    let (_dir, state, app) = quota_fixture().await;
    let first = post(&app, payload("lost-response", true)).await;
    fill_quota(&state).await;
    let replay = post(&app, payload("  lost-response  ", true)).await;
    assert_eq!(replay["conversation_id"], first["conversation_id"]);
    assert_eq!(replay["status"], "captured");
    assert_eq!(replay["deduplicated"], true);
    assert!(replay.get("job_id").is_none());
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
async fn full_quota_replays_processed_and_running_receipts_without_new_jobs() {
    for processed in [false, true] {
        let (_dir, state, app) = quota_fixture().await;
        let conversation = ConversationRecord {
            id: "accepted-conversation".into(),
            user_id: "token-user".into(),
            source: "chatgpt".into(),
            url: "https://chatgpt.com/c/quota".into(),
            title: None,
            raw_content: "User: save a database decision".into(),
            metadata: json!({}),
            captured_at: "2026-10-06T10:00:00Z".into(),
            created_at: "2026-10-06T10:00:00Z".into(),
            status: if processed {
                ConversationStatus::Processed
            } else {
                ConversationStatus::Processing
            },
            idempotency_key: "accepted-key".into(),
            item_ids: vec![],
            last_error: None,
            superseded_by: None,
        };
        state
            .conversation_repo
            .insert_or_fetch_conversation_by_idempotency(&conversation)
            .await
            .unwrap();
        let job = ExtractionJobRecord {
            id: "accepted-job".into(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: if processed {
                JobStatus::Succeeded
            } else {
                JobStatus::Running
            },
            created_at: "2026-10-06T10:00:00Z".into(),
            updated_at: "2026-10-06T10:00:01Z".into(),
            error: None,
            attempt_count: 1,
            lease_owner: Some("fixture-owner".into()),
            lease_expires_at: Some("2099-01-01T00:00:00Z".into()),
        };
        state.job_repo.upsert_job(&job).await.unwrap();
        fill_quota(&state).await;
        let replay = post(&app, payload("accepted-key", false)).await;
        assert_eq!(replay["conversation_id"], conversation.id);
        assert_eq!(replay["deduplicated"], true);
        if processed {
            assert_eq!(replay["status"], "processed");
            assert_eq!(replay["job_id"], job.id);
        } else {
            assert_eq!(replay["status"], "processing");
            assert_eq!(replay["job_id"], job.id);
        }
        let persisted = state
            .job_repo
            .find_job_by_id(&job.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted.status, job.status);
        assert_eq!(persisted.attempt_count, 1);
        assert_eq!(
            state
                .conversation_repo
                .count_conversations(None)
                .await
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn full_quota_still_rejects_new_keys_without_persisting_them() {
    let (_dir, state, app) = quota_fixture().await;
    fill_quota(&state).await;
    for ingest_only in [false, true] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/conversations")
                    .header(header::AUTHORIZATION, "Bearer test-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(payload("new-key", ingest_only).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    assert_eq!(
        state
            .conversation_repo
            .count_conversations(None)
            .await
            .unwrap(),
        0
    );
    assert!(state
        .job_repo
        .list_recoverable_jobs(&crate::models::now_iso(), 100)
        .await
        .unwrap()
        .is_empty());
}
