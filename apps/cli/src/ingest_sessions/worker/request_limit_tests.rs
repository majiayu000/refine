use super::*;
use async_trait::async_trait;
use refine_core::error::InfraResult;
use refine_core::infra::{LlmRunBudget, SqliteStore};
use refine_core::session::FACET_REQUEST_FRAMING_BYTES;
use refine_core::session::{
    chunk_session, needs_chunking, MessageProvenance, MessageRole, Session, SessionMessage,
    SessionMeta,
};
use std::collections::VecDeque;
use std::path::PathBuf;

struct RecordingClient {
    requests: Mutex<Vec<(String, String)>>,
    responses: Mutex<VecDeque<String>>,
    ledger: Option<PathBuf>,
    budget: LlmRunBudget,
    transient_first: AtomicBool,
}

impl RecordingClient {
    fn new(responses: Vec<String>, ledger: Option<PathBuf>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
            ledger,
            budget: LlmRunBudget::default(),
            transient_first: AtomicBool::new(false),
        }
    }

    fn calls(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[async_trait]
impl LlmClient for RecordingClient {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
        self.requests
            .lock()
            .unwrap()
            .push((prompt.to_string(), system.unwrap_or_default().to_string()));
        if self.transient_first.swap(false, Ordering::Relaxed) {
            return Err(InfraError::LlmTransport("synthetic disconnect".to_string()));
        }
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| valid_response("synthetic summary")))
    }

    fn usage_ledger_path(&self) -> InfraResult<Option<PathBuf>> {
        Ok(self.ledger.clone())
    }

    fn run_budget(&self) -> Option<&LlmRunBudget> {
        Some(&self.budget)
    }
}

fn valid_response(summary: &str) -> String {
    serde_json::json!({
        "session_summary": summary,
        "cognitive_level": "unknown",
        "collaboration_mode": "unknown"
    })
    .to_string()
}

fn synthetic_session(messages: Vec<SessionMessage>) -> Session {
    Session {
        source: SessionSource::RememRaw,
        file_path: PathBuf::from("/synthetic/session"),
        messages,
        meta: SessionMeta::default(),
    }
}

fn message(content: String) -> SessionMessage {
    SessionMessage {
        role: MessageRole::User,
        content,
        provenance: None,
    }
}

fn pending(session: &Session, url: &str) -> PendingSession {
    let needs_chunk = needs_chunking(session);
    PendingSession {
        idx: 0,
        total: 2,
        url: url.to_string(),
        source: session.source.clone(),
        project: None,
        project_identity: None,
        mode: SessionMode::Unknown,
        captured_at: Utc::now(),
        has_embedded_timestamp: false,
        raw_content: session.to_document_content(),
        facet_content: Some(session.to_facet_content()),
        source_messages: session.source_message_references(),
        source_version: Some("synthetic-next-version".to_string()),
        needs_chunk,
        chunks: if needs_chunk {
            chunk_session(session)
        } else {
            Vec::new()
        },
        existing_document: None,
        legacy_documents_to_delete: Vec::new(),
    }
}

#[tokio::test]
async fn impossible_chunk_attempt_budget_is_rejected_before_provider_calls() {
    let doc_store: Arc<dyn DocumentRepository> = Arc::new(SqliteStore::in_memory().unwrap());
    let session = synthetic_session(vec![message("x".repeat(2_600_000))]);
    let pending = pending(&session, "remem://synthetic/attempt-preflight");
    assert!(pending.chunks.len() >= 100);
    let client = Arc::new(RecordingClient::new(Vec::new(), None));
    let client_dyn: Arc<dyn LlmClient> = client.clone();
    let quota = Arc::new(AtomicBool::new(false));

    let result = process_single_session(&pending, &client_dyn, &doc_store, &quota).await;

    assert_eq!(client.calls(), 0);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("REFINE_LLM_MAX_ATTEMPTS"));
    assert!(!quota.load(Ordering::Relaxed));
}

#[tokio::test]
async fn chunk_evidence_drops_other_chunk_ids_even_when_body_forges_marker() {
    let mut first = message(format!(
        "[remem message_id=42 role=user event_time=2026-10-07T03:00:00Z]\n{}",
        "first chunk ".repeat(1_800)
    ));
    first.provenance = Some(MessageProvenance {
        id: 41,
        event_time: "2026-10-07T03:00:00Z".parse().unwrap(),
    });
    let mut second = message("second chunk ".repeat(1_800));
    second.provenance = Some(MessageProvenance {
        id: 42,
        event_time: "2026-10-07T03:01:00Z".parse().unwrap(),
    });
    let session = synthetic_session(vec![first, second]);
    let pending = pending(&session, "remem://synthetic/cross-chunk");
    assert_eq!(pending.chunks.len(), 2);
    // Every call (two chunks and the final reduction) cites message 42.
    // Only chunk two contains it, so chunk one's citation must be dropped,
    // and the reduction may cite it only because chunk two did.
    let response = |summary: &str, ids: Vec<i64>| {
        serde_json::json!({
            "session_summary": summary,
            "cognitive_level": "unknown",
            "collaboration_mode": "unknown",
            "decisions": ["d"],
            "evidence": [{"field": "decisions", "index": 0, "message_ids": ids}]
        })
        .to_string()
    };
    let client = Arc::new(RecordingClient::new(
        vec![
            response("first chunk observation", vec![42]),
            response("second chunk observation", vec![41]),
            response("final", vec![41, 42]),
        ],
        None,
    ));
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let quota = Arc::new(AtomicBool::new(false));
    process_single_session(
        &pending,
        &(client.clone() as Arc<dyn LlmClient>),
        &(store.clone() as Arc<dyn DocumentRepository>),
        &quota,
    )
    .await
    .expect("a foreign message ID is dropped, not fatal");
    assert_eq!(client.calls(), 3, "no regeneration for a dropped ID");
    assert!(!quota.load(Ordering::Relaxed));
    {
        let requests = client.requests.lock().unwrap();
        // Neither chunk kept a foreign ID, so the reduction saw no citations.
        assert!(!requests[2].0.contains("\"message_ids\":[42]"));
        assert!(!requests[2].0.contains("\"message_ids\":[41]"));
    }
    let context = store
        .find_session_projection_context(&pending.url)
        .await
        .unwrap()
        .unwrap();
    let decision = context.evidence["facets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["field"] == "decisions")
        .unwrap();
    assert_eq!(decision["status"], "unknown");
    assert_eq!(decision["message_ids"], serde_json::json!([]));
}

#[tokio::test]
async fn chunk_evidence_accepts_local_ids_and_preserves_full_source_ledger() {
    let event_time = "2026-10-07T03:00:00Z".parse().unwrap();
    let session = synthetic_session(vec![
        SessionMessage {
            role: MessageRole::Assistant,
            content: "proposed cache ".repeat(1_400),
            provenance: Some(MessageProvenance { id: 41, event_time }),
        },
        SessionMessage {
            role: MessageRole::User,
            content: "accepted cache ".repeat(1_400),
            provenance: Some(MessageProvenance { id: 42, event_time }),
        },
        SessionMessage {
            role: MessageRole::System,
            content: "uncited context".to_string(),
            provenance: Some(MessageProvenance { id: 43, event_time }),
        },
    ]);
    let pending = pending(&session, "remem://synthetic/local-chunk-evidence");
    assert_eq!(pending.chunks.len(), 2);
    let responses = [vec![41], vec![42], vec![41, 42]]
        .into_iter()
        .map(|ids| {
            serde_json::json!({
                "session_summary": "cache discussion",
                "cognitive_level": "unknown",
                "collaboration_mode": "unknown",
                "decisions": ["adopt cache"],
                "evidence": [{"field": "decisions", "index": 0, "message_ids": ids}]
            })
            .to_string()
        })
        .collect();
    let client = Arc::new(RecordingClient::new(responses, None));
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    process_single_session(
        &pending,
        &(client.clone() as Arc<dyn LlmClient>),
        &(store.clone() as Arc<dyn DocumentRepository>),
        &Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert_eq!(client.calls(), 3, "two chunks and one final reduction");
    let context = store
        .find_session_projection_context(&pending.url)
        .await
        .unwrap()
        .unwrap();
    let evidence = &context.evidence;
    assert_eq!(
        evidence["source_messages"],
        serde_json::to_value(session.source_message_references()).unwrap()
    );
    let decision = evidence["facets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["field"] == "decisions")
        .unwrap();
    assert_eq!(decision["message_ids"], serde_json::json!([41, 42]));
    assert_eq!(decision["status"], "references_validated");
    let requests = client.requests.lock().unwrap();
    assert!(requests[0].0.contains("message_id=41"));
    assert!(!requests[0].0.contains("message_id=42"));
    assert!(!requests[1].0.contains("message_id=41"));
    assert!(requests[1].0.contains("message_id=42"));
    assert!(requests[2].0.contains("\"message_ids\":[41]"));
    assert!(requests[2].0.contains("\"message_ids\":[42]"));
}

#[tokio::test]
async fn facet_request_exact_limit_counts_template_system_roles_and_provenance() {
    let overhead =
        build_facet_prompt("").len() + FACET_SYSTEM_PROMPT.len() + FACET_REQUEST_FRAMING_BYTES;
    for role in [
        MessageRole::User,
        MessageRole::Assistant,
        MessageRole::System,
    ] {
        for with_provenance in [false, true] {
            for multibyte in [false, true] {
                let mut source = SessionMessage {
                    role: role.clone(),
                    content: String::new(),
                    provenance: with_provenance.then(|| MessageProvenance {
                        id: 42,
                        event_time: "2026-10-07T03:00:00Z".parse().unwrap(),
                    }),
                };
                let empty = synthetic_session(vec![source.clone()]).to_facet_content();
                let remaining = FACET_REQUEST_MAX_BYTES - overhead - empty.len();
                source.content = if multibyte {
                    format!(
                        "{}{}",
                        "你".repeat(remaining / 3),
                        "x".repeat(remaining % 3)
                    )
                } else {
                    "x".repeat(remaining)
                };
                let original = synthetic_session(vec![source.clone()]);
                let content = original.to_facet_content();
                let chunks = chunk_session(&original);
                assert_eq!(chunks.len(), 1, "whole message must not split");
                assert_eq!(chunks[0].content, content);
                let client = Arc::new(RecordingClient::new(Vec::new(), None));
                let client_dyn: Arc<dyn LlmClient> = client.clone();
                let quota = Arc::new(AtomicBool::new(false));
                llm_call_with_retry(&client_dyn, &content, &quota, "session extraction")
                    .await
                    .unwrap();
                {
                    let requests = client.requests.lock().unwrap();
                    assert_eq!(requests[0].0, build_facet_prompt(&content));
                    assert_eq!(requests[0].1, FACET_SYSTEM_PROMPT);
                    assert_eq!(
                        requests[0].0.len() + requests[0].1.len() + FACET_REQUEST_FRAMING_BYTES,
                        FACET_REQUEST_MAX_BYTES
                    );
                }

                source.content.push('x');
                let oversized = synthetic_session(vec![source]).to_facet_content();
                let error = extract_and_parse_facets_with_retry_policy(
                    &oversized,
                    &client_dyn,
                    &quota,
                    2,
                    0,
                )
                .await
                .unwrap_err();
                assert!(
                    matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { request_bytes, limit_bytes, stage: "session extraction" }) if *request_bytes == FACET_REQUEST_MAX_BYTES + 1 && *limit_bytes == FACET_REQUEST_MAX_BYTES)
                );
                assert_eq!(
                    client.calls(),
                    1,
                    "L + 1 must never reach provider or regenerate"
                );
                assert!(!quota.load(Ordering::Relaxed));
                assert!(!error.to_string().contains(&content));
            }
        }
    }
}

#[tokio::test]
async fn facet_request_soft_target_and_activation_edges_remain_accepted() {
    let client = Arc::new(RecordingClient::new(Vec::new(), None));
    let client_dyn: Arc<dyn LlmClient> = client.clone();
    let quota = Arc::new(AtomicBool::new(false));
    for bytes in [25_000, 25_001, 29_999, 30_000, 30_001] {
        let session = synthetic_session(vec![message("x".repeat(bytes - "User: \n".len()))]);
        assert_eq!(session.to_facet_content().len(), bytes);
        assert_eq!(needs_chunking(&session), bytes > 30_000);
        assert_eq!(
            chunk_session(&session)[0].content,
            session.to_facet_content()
        );
        llm_call_with_retry(
            &client_dyn,
            &session.to_facet_content(),
            &quota,
            "session extraction",
        )
        .await
        .unwrap();
    }
    assert_eq!(client.calls(), 5);
}

#[tokio::test]
async fn facet_request_regeneration_and_provider_retry_keep_the_same_bounded_input() {
    let overhead =
        build_facet_prompt("").len() + FACET_SYSTEM_PROMPT.len() + FACET_REQUEST_FRAMING_BYTES;
    let content = "x".repeat(FACET_REQUEST_MAX_BYTES - overhead);
    let client = Arc::new(RecordingClient::new(
        vec![
            "synthetic invalid JSON".to_string(),
            valid_response("regenerated"),
        ],
        None,
    ));
    client.transient_first.store(true, Ordering::Relaxed);
    let quota = Arc::new(AtomicBool::new(false));
    let result = extract_and_parse_facets_with_retry_policy(
        &content,
        &(client.clone() as Arc<dyn LlmClient>),
        &quota,
        2,
        0,
    )
    .await
    .unwrap();
    assert_eq!(result.session_summary, "regenerated");
    assert_eq!(client.calls(), 3);
    for (prompt, system) in client.requests.lock().unwrap().iter() {
        assert_eq!(prompt, &build_facet_prompt(&content));
        assert_eq!(system, FACET_SYSTEM_PROMPT);
        assert_eq!(
            prompt.len() + system.len() + FACET_REQUEST_FRAMING_BYTES,
            FACET_REQUEST_MAX_BYTES
        );
    }
}

#[tokio::test]
async fn oversized_message_is_read_in_pieces_instead_of_rejected() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let session = synthetic_session(vec![
        message("x".repeat(25_000)),
        message("private-synthetic-marker\n".repeat(3_000)),
    ]);
    let pending = pending(&session, "remem://synthetic/oversized");
    assert_eq!(
        pending.chunks.len(),
        5,
        "one whole message plus line-aligned pieces"
    );
    for chunk in &pending.chunks {
        assert!(checked_facet_prompt(&chunk.content, "chunk extraction").is_ok());
    }
    let client = Arc::new(RecordingClient::new(Vec::new(), None));
    let quota = Arc::new(AtomicBool::new(false));
    process_single_session(
        &pending,
        &(client.clone() as Arc<dyn LlmClient>),
        &doc_store,
        &quota,
    )
    .await
    .unwrap();
    assert_eq!(client.calls(), 6, "five chunks and one final reduction");
    assert!(store.find_by_url(&pending.url).await.unwrap().is_some());
}

#[tokio::test]
async fn oversized_final_reduction_is_reduced_in_levels_within_the_limit() {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let chunk_response = valid_response(&"synthetic-private-summary".repeat(1_000));
    let final_response = serde_json::json!({
        "session_summary": "reduced",
        "cognitive_level": "unknown",
        "collaboration_mode": "unknown",
        "decisions": ["kept across levels"]
    })
    .to_string();
    let mut responses = vec![chunk_response; 3];
    responses.extend([
        valid_response("group one"),
        valid_response("group two"),
        final_response,
    ]);
    let client = Arc::new(RecordingClient::new(responses, None));
    let session = synthetic_session((0..3).map(|_| message("x".repeat(20_000))).collect());
    let pending = pending(&session, "remem://synthetic/levels");
    assert_eq!(pending.chunks.len(), 3);
    let quota = Arc::new(AtomicBool::new(false));
    process_single_session(
        &pending,
        &(client.clone() as Arc<dyn LlmClient>),
        &doc_store,
        &quota,
    )
    .await
    .unwrap();
    let requests = client.requests.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        6,
        "three chunks, two group reductions, one final"
    );
    for (prompt, system) in &requests {
        assert!(
            prompt.len() + system.len() + FACET_REQUEST_FRAMING_BYTES <= FACET_REQUEST_MAX_BYTES
        );
    }
    let first_partial = serde_json::to_string(
        &parse_facet_response(&valid_response(&"synthetic-private-summary".repeat(1_000))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        requests[3].0,
        build_facet_prompt(&[first_partial.clone(), first_partial].join(REDUCTION_SEPARATOR)),
        "groups pack consecutive partials up to the limit"
    );
    let document = store.find_by_url(&pending.url).await.unwrap().unwrap();
    let items = store
        .find_items_by_document_id(document.id())
        .await
        .unwrap();
    assert!(!items.is_empty());
    assert!(items.iter().all(|item| item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "facet_reduction_depth_2")));
}

#[tokio::test]
async fn facet_request_final_reduction_rejects_without_partial_publish_and_keeps_spent_calls() {
    let tmp = tempfile::tempdir().unwrap();
    let ledger = tmp.path().join("usage.jsonl");
    // Each partial fits alone but no two fit together: the levels cannot shrink.
    let response = valid_response(&"synthetic-private-summary".repeat(1_400));
    let normalized = serde_json::to_string(&parse_facet_response(&response).unwrap()).unwrap();
    assert!(checked_facet_prompt(&normalized, "chunk extraction").is_ok());
    assert!(checked_facet_prompt(
        &[normalized.clone(), normalized.clone()].join(REDUCTION_SEPARATOR),
        "group reduction"
    )
    .is_err());
    let reduction = [normalized.clone(), normalized.clone(), normalized].join("\n\n---\n\n");
    let expected_bytes = build_facet_prompt(&reduction).len()
        + FACET_SYSTEM_PROMPT.len()
        + FACET_REQUEST_FRAMING_BYTES;
    assert!(expected_bytes > FACET_REQUEST_MAX_BYTES);
    let client = Arc::new(RecordingClient::new(
        vec![response; 3],
        Some(ledger.clone()),
    ));
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let session = synthetic_session((0..3).map(|_| message("x".repeat(20_000))).collect());
    let pending = pending(&session, "remem://synthetic/reduction");
    assert_eq!(pending.chunks.len(), 3);
    let quota = Arc::new(AtomicBool::new(false));
    let error = process_single_session(
        &pending,
        &(client.clone() as Arc<dyn LlmClient>),
        &doc_store,
        &quota,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(error.downcast_ref::<InfraError>(), Some(InfraError::FacetRequestTooLarge { stage: "final reduction", request_bytes, .. }) if *request_bytes == expected_bytes)
    );
    assert_eq!(client.calls(), 3);
    assert!(!quota.load(Ordering::Relaxed));
    assert!(store.find_by_url(&pending.url).await.unwrap().is_none());
    assert!(store
        .find_session_projection_versions()
        .await
        .unwrap()
        .is_empty());
    let usage = std::fs::read_to_string(ledger).unwrap();
    assert_eq!(usage.lines().count(), 3);
    assert!(!usage.contains("synthetic-private-summary"));
    let (code, diagnostic) = content_rejection(&error).unwrap();
    assert_eq!(code, "facet_request_too_large");
    assert!(diagnostic.contains("已完成 3 个分块"));
    assert!(diagnostic.contains(&expected_bytes.to_string()));
    assert!(!diagnostic.contains("synthetic-private-summary"));
}

#[tokio::test]
async fn retried_quarantine_that_fails_transiently_is_released_for_routine_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("quarantine.jsonl");
    let session = synthetic_session(vec![message("ordinary synthetic input".to_string())]);
    let pending = pending(&session, "remem://synthetic/retried");
    let mut quarantine = QuarantineStore::load_from(path.clone()).unwrap();
    quarantine.record(
        &pending.url,
        pending.source_version.as_deref(),
        "facet_request_too_large",
        "rejected by an older build",
    );
    quarantine.save_if_dirty().unwrap();
    drop(quarantine);
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let client: Arc<dyn LlmClient> = Arc::new(RecordingClient::new(
        vec!["not json".to_string(), "still not json".to_string()],
        None,
    ));
    let error = process_pending_sessions(
        vec![pending],
        0,
        0,
        0,
        0,
        HashSet::new(),
        false,
        true,
        Some(QuarantineStore::load_from(path.clone()).unwrap()),
        store,
        Some(client),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("瞬态失败 1"));
    assert_eq!(QuarantineStore::load_from(path).unwrap().len(), 0);
}

#[tokio::test]
async fn facet_request_quarantine_keeps_batch_incomplete_but_allows_unrelated_session() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("quarantine.jsonl");
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let doc_store: Arc<dyn DocumentRepository> = store.clone();
    let big = valid_response(&"private-synthetic-marker".repeat(1_400));
    let client = Arc::new(RecordingClient::new(vec![big.clone(), big], None));
    let client_dyn: Arc<dyn LlmClient> = client.clone();
    // Two chunks whose partials cannot be reduced together within the limit.
    let oversized = synthetic_session(vec![
        message("private-synthetic-marker".repeat(1_000)),
        message("private-synthetic-marker".repeat(1_000)),
    ]);
    let normal = synthetic_session(vec![message("small synthetic input".to_string())]);
    let error = process_pending_sessions(
        vec![
            pending(&oversized, "remem://synthetic/large"),
            pending(&normal, "remem://synthetic/normal"),
        ],
        0,
        0,
        0,
        0,
        HashSet::new(),
        false,
        false,
        Some(QuarantineStore::load_from(path.clone()).unwrap()),
        doc_store.clone(),
        Some(client_dyn.clone()),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("摄入不完整"));
    assert_eq!(
        client.calls(),
        3,
        "two chunks spent, no reduction sent, one normal session"
    );
    assert!(store
        .find_by_url("remem://synthetic/normal")
        .await
        .unwrap()
        .is_some());
    assert!(store
        .find_by_url("remem://synthetic/large")
        .await
        .unwrap()
        .is_none());
    let record = std::fs::read_to_string(&path).unwrap();
    assert_eq!(record.lines().count(), 1);
    assert!(record.contains("facet_request_too_large"));
    assert!(!record.contains("private-synthetic-marker"));
    let skipped = process_pending_sessions(
        vec![pending(&oversized, "remem://synthetic/large")],
        0,
        0,
        0,
        0,
        HashSet::new(),
        false,
        false,
        Some(QuarantineStore::load_from(path).unwrap()),
        doc_store,
        Some(client_dyn),
    )
    .await;
    assert!(
        skipped.is_err(),
        "quarantined selection cannot report successful completion"
    );
    assert_eq!(
        client.calls(),
        3,
        "unchanged quarantined input is not automatically retried"
    );
}
