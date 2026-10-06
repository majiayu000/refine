use async_trait::async_trait;
use refine_core::error::InfraResult;
use refine_core::infra::LlmClient;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::models::{ConversationRecord, ConversationStatus, ExtractionJobRecord};
use crate::state::AuthConfig;

use super::*;

struct OrderedResponses {
    old_started: Semaphore,
    release_old: Semaphore,
}

#[async_trait]
impl LlmClient for OrderedResponses {
    async fn complete(&self, prompt: &str, _system: Option<&str>) -> InfraResult<String> {
        let old = prompt.contains("slow-old-capture");
        if old {
            self.old_started.add_permits(1);
            self.release_old.acquire().await.unwrap().forget();
        }
        Ok(json!({"items": [{
            "type": "knowledge", "title": if old { "Stale old result" } else { "Current new result" },
            "summary": "Synthetic regression", "content": "Synthetic evidence", "tags": [],
        }]}).to_string())
    }
}

async fn enqueue(state: &AppState, key: &str, raw_content: &str) -> (String, String) {
    let now = now_iso();
    let conversation = ConversationRecord {
        id: format!("conversation-{key}"),
        user_id: "test-user".into(),
        source: "chatgpt".into(),
        url: "https://chatgpt.com/c/concurrent".into(),
        title: Some(key.into()),
        raw_content: raw_content.into(),
        metadata: json!({}),
        captured_at: now.clone(),
        created_at: now.clone(),
        status: ConversationStatus::Queued,
        idempotency_key: key.into(),
        item_ids: vec![],
        last_error: None,
    };
    let job = ExtractionJobRecord {
        id: format!("job-{key}"),
        conversation_id: conversation.id.clone(),
        mode: ExtractionMode::Auto,
        status: JobStatus::Pending,
        created_at: now.clone(),
        updated_at: now,
        error: None,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
    };
    state
        .conversation_repo
        .insert_or_fetch_conversation_with_job(&conversation, &job)
        .await
        .unwrap();
    (conversation.id, job.id)
}

#[tokio::test]
async fn late_model_result_gets_durable_superseded_receipt_and_preserves_newer_items() {
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(OrderedResponses {
        old_started: Semaphore::new(0),
        release_old: Semaphore::new(0),
    });
    let mut state = AppState::build_for_test(
        directory.path().join("publication.sqlite"),
        AuthConfig {
            api_token: None,
            dev_anon: true,
        },
    )
    .await
    .unwrap();
    state.llm_client = Some(provider.clone());
    let state = Arc::new(state);
    let (old_capture, old_job) = enqueue(
        &state,
        "old",
        "User: slow-old-capture\nAssistant: old answer",
    )
    .await;
    let (new_capture, new_job) = enqueue(
        &state,
        "new",
        "User: current capture\nAssistant: new answer",
    )
    .await;

    let old_state = state.clone();
    let old_id = old_capture.clone();
    let old_job_id = old_job.clone();
    let old = tokio::spawn(async move {
        run_extraction(old_state, &old_id, &old_job_id, ExtractionMode::Auto).await
    });
    tokio::time::timeout(Duration::from_secs(5), provider.old_started.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    run_extraction(state.clone(), &new_capture, &new_job, ExtractionMode::Auto)
        .await
        .unwrap();
    provider.release_old.add_permits(1);
    let error = old.await.unwrap().unwrap_err();
    assert!(error.contains("capture_superseded"), "{error}");

    let job = state
        .job_repo
        .find_job_by_id(&old_job)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert!(job.error.as_deref().unwrap().contains("capture_superseded"));
    let capture = state
        .conversation_repo
        .find_conversation_by_id(&old_capture)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(capture.status, ConversationStatus::Failed);
    assert_eq!(capture.last_error, job.error);
    assert!(capture.item_ids.is_empty());
    assert_eq!(
        state
            .job_repo
            .find_job_by_id(&new_job)
            .await
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Succeeded
    );
    let doc = state
        .doc_store
        .find_by_url("https://chatgpt.com/c/concurrent")
        .await
        .unwrap()
        .unwrap();
    assert!(doc.raw_content().contains("current capture"));
    let items = state.store.find_by_document_id(doc.id()).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title(), "Current new result");
    assert!(state
        .job_repo
        .list_recoverable_jobs("2099-01-01T00:00:00Z", 10)
        .await
        .unwrap()
        .is_empty());
}
