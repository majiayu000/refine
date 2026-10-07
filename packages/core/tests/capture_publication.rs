use refine_core::conversation::{
    ConversationRecord, ConversationRepository, ConversationStatus, ExtractionJobRecord,
    ExtractionMode, JobPublicationOutcome, JobRepository, JobStatus,
};
use refine_core::error::InfraError;
use refine_core::infra::{configure_sqlite_connection, SqliteStore};
use refine_core::knowledge::{Document, DocumentRepository, Item, ItemRepository};
use rusqlite::Connection;
use serde_json::json;

const URL: &str = "https://example.test/conversation";
const NOW: &str = "2026-10-06T12:00:00Z";

fn capture(id: &str, raw: &str) -> (ConversationRecord, ExtractionJobRecord, Document, Item) {
    let conversation = ConversationRecord {
        id: id.into(),
        user_id: "owner".into(),
        source: "browser".into(),
        url: URL.into(),
        title: Some(id.into()),
        raw_content: raw.into(),
        metadata: json!({}),
        captured_at: NOW.into(),
        created_at: NOW.into(),
        status: ConversationStatus::Queued,
        idempotency_key: format!("key-{id}"),
        item_ids: Vec::new(),
        last_error: None,
        superseded_by: None,
    };
    let job = ExtractionJobRecord {
        id: format!("job-{id}"),
        conversation_id: id.into(),
        mode: ExtractionMode::Auto,
        status: JobStatus::Pending,
        created_at: NOW.into(),
        updated_at: NOW.into(),
        error: None,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
    };
    let mut document = Document::new("browser", raw);
    document.set_url(URL);
    document.set_title(id);
    document.set_captured_at(NOW.parse().unwrap());
    let mut item = Item::new_knowledge(id, raw);
    item.set_document_id(document.id().clone());
    (conversation, job, document, item)
}

async fn admit_and_claim(
    store: &SqliteStore,
    conversation: &ConversationRecord,
    job: &ExtractionJobRecord,
) {
    store
        .insert_or_fetch_conversation_with_quota(conversation, Some(job), None)
        .await
        .unwrap();
    assert!(store
        .claim_job(&job.id, "worker", NOW, "2099-01-01T00:00:00Z")
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn later_accepted_capture_wins_across_connections_and_replay_retains_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures.sqlite");
    let first = SqliteStore::open(&path).unwrap();
    let second = SqliteStore::open(&path).unwrap();
    let (old, old_job, old_doc, old_item) = capture("old", "old snapshot");
    let (mut new, new_job, mut new_doc, new_item) = capture("new", "new snapshot");
    // A delayed offline upload is ordered by server acceptance, even with an
    // older client clock. Equal timestamps are covered by the next test.
    new.captured_at = "2000-01-01T00:00:00Z".into();
    new_doc.set_captured_at(new.captured_at.parse().unwrap());
    admit_and_claim(&first, &old, &old_job).await;
    admit_and_claim(&second, &new, &new_job).await;
    assert_eq!(
        second
            .finish_job_claim_with_results(
                &new_job.id,
                "worker",
                &new_doc,
                &[new_item.clone()],
                NOW
            )
            .await
            .unwrap(),
        JobPublicationOutcome::Published
    );
    assert_eq!(
        first
            .finish_job_claim_with_results(
                &old_job.id,
                "worker",
                &old_doc,
                &[old_item.clone()],
                NOW
            )
            .await
            .unwrap(),
        JobPublicationOutcome::Superseded
    );
    let document = DocumentRepository::find_by_url(&first, URL)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(document.raw_content(), "new snapshot");
    assert_eq!(document.captured_at(), new_doc.captured_at());
    let items = ItemRepository::find_all(&first).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id(), new_item.id());
    let old_receipt = first
        .find_conversation_by_id(&old.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old_receipt.status, ConversationStatus::Processed);
    assert_eq!(old_receipt.superseded_by.as_deref(), Some(new.id.as_str()));
    assert!(old_receipt.item_ids.is_empty());
    assert_eq!(
        first
            .find_job_by_id(&old_job.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Succeeded
    );
    let new_receipt = first
        .find_conversation_by_id(&new.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(new_receipt.item_ids, vec![new_item.id().to_string()]);
    assert!(new_receipt.superseded_by.is_none());
    let mut replay = old.clone();
    replay.id = "replay-id".into();
    let (receipt, job) = second
        .insert_or_fetch_conversation_with_quota(&replay, Some(&old_job), Some(1))
        .await
        .unwrap();
    assert_eq!(receipt.id, old.id);
    assert_eq!(job.unwrap().id, old_job.id);
    assert_eq!(receipt.superseded_by, old_receipt.superseded_by);
    let conn = Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM capture_revisions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn newer_pending_work_keeps_last_successful_result_until_atomic_publication() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    let (old, old_job, old_doc, old_item) = capture("old", "old snapshot");
    let (new, new_job, new_doc, new_item) = capture("new", "new snapshot");
    admit_and_claim(&store, &old, &old_job).await;
    admit_and_claim(&store, &new, &new_job).await;
    assert_eq!(
        store
            .finish_job_claim_with_results(
                &old_job.id,
                "worker",
                &old_doc,
                &[old_item.clone()],
                NOW
            )
            .await
            .unwrap(),
        JobPublicationOutcome::Published
    );

    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_new_item BEFORE INSERT ON items WHEN NEW.title = 'new' BEGIN SELECT RAISE(ABORT, 'injected publication failure'); END;").unwrap();
    assert!(store
        .finish_job_claim_with_results(&new_job.id, "worker", &new_doc, &[new_item.clone()], NOW)
        .await
        .is_err());
    assert_eq!(
        DocumentRepository::find_by_url(&store, URL)
            .await
            .unwrap()
            .unwrap()
            .raw_content(),
        "old snapshot"
    );
    let old_receipt = store
        .find_conversation_by_id(&old.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old_receipt.item_ids, vec![old_item.id().to_string()]);
    assert!(old_receipt.superseded_by.is_none());
    assert_eq!(
        store
            .find_job_by_id(&new_job.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        JobStatus::Running
    );
    conn.execute_batch("DROP TRIGGER fail_new_item").unwrap();

    assert_eq!(
        store
            .finish_job_claim_with_results(
                &new_job.id,
                "wrong-owner",
                &new_doc,
                &[new_item.clone()],
                NOW
            )
            .await
            .unwrap(),
        JobPublicationOutcome::LostClaim
    );
    assert_eq!(
        store
            .finish_job_claim_with_results(&new_job.id, "worker", &new_doc, &[new_item], NOW)
            .await
            .unwrap(),
        JobPublicationOutcome::Published
    );
    let old_receipt = store
        .find_conversation_by_id(&old.id)
        .await
        .unwrap()
        .unwrap();
    assert!(old_receipt.item_ids.is_empty());
    assert_eq!(old_receipt.superseded_by.as_deref(), Some("new"));
    assert!(ItemRepository::find_by_id(&store, old_item.id())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn acceptance_high_water_mark_survives_receipt_deletion_and_vacuum() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("captures.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    let (old, old_job, old_doc, old_item) = capture("old", "old snapshot");
    admit_and_claim(&store, &old, &old_job).await;
    store
        .finish_job_claim_with_results(&old_job.id, "worker", &old_doc, &[old_item], NOW)
        .await
        .unwrap();
    let conn = Connection::open(&path).unwrap();
    configure_sqlite_connection(&conn).unwrap();
    conn.execute_batch("DELETE FROM conversations; VACUUM;")
        .unwrap();
    drop(conn);
    drop(store);
    let reopened = SqliteStore::open(&path).unwrap();
    let (new, new_job, new_doc, new_item) = capture("new", "new snapshot");
    admit_and_claim(&reopened, &new, &new_job).await;
    assert_eq!(
        reopened
            .finish_job_claim_with_results(&new_job.id, "worker", &new_doc, &[new_item], NOW)
            .await
            .unwrap(),
        JobPublicationOutcome::Published
    );
    let conn = Connection::open(path).unwrap();
    assert!(
        conn.query_row(
            "SELECT revision FROM capture_revisions WHERE conversation_id = 'new'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap()
            > 1
    );
}

#[tokio::test]
async fn legacy_upgrade_preserves_current_document_without_guessing_acceptance_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let store = SqliteStore::open(&path).unwrap();
    let (old, old_job, old_doc, old_item) = capture("old", "old snapshot");
    let (new, new_job, new_doc, new_item) = capture("new", "new snapshot");
    admit_and_claim(&store, &old, &old_job).await;
    admit_and_claim(&store, &new, &new_job).await;
    store
        .finish_job_claim_with_results(&new_job.id, "worker", &new_doc, &[new_item], NOW)
        .await
        .unwrap();
    drop(store);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TRIGGER conversations_capture_revision;
        DROP TRIGGER conversations_capture_source_revision;
        DROP TABLE capture_revisions; DROP TABLE document_capture_publications;
        DROP TABLE capture_publication_context;
        ALTER TABLE extraction_jobs DROP COLUMN source_revision;
        ALTER TABLE conversations DROP COLUMN superseded_by;",
    )
    .unwrap();
    drop(conn);
    let upgraded = SqliteStore::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM capture_revisions", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    assert_eq!(
        DocumentRepository::find_by_url(&upgraded, URL)
            .await
            .unwrap()
            .unwrap()
            .raw_content(),
        "new snapshot"
    );
    assert_eq!(
        upgraded
            .finish_job_claim_with_results(&old_job.id, "worker", &old_doc, &[old_item], NOW)
            .await
            .unwrap(),
        JobPublicationOutcome::Published
    );
    drop(upgraded);
    let reopened = SqliteStore::open(path).unwrap();
    assert_eq!(
        reopened
            .find_conversation_by_id("old")
            .await
            .unwrap()
            .unwrap()
            .superseded_by
            .as_deref(),
        None
    );
}

#[tokio::test]
async fn admission_replays_pending_work_at_quota_but_rejects_new_or_conflicting_requests() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.sqlite");
    let first = SqliteStore::open(&path).unwrap();
    let second = SqliteStore::open(&path).unwrap();
    let (conversation, job, _, _) = capture("accepted", "same request");
    let mut duplicate = conversation.clone();
    duplicate.id = "duplicate".into();
    let mut duplicate_job = job.clone();
    duplicate_job.id = "duplicate-job".into();
    duplicate_job.conversation_id = duplicate.id.clone();
    let (a, b) = tokio::join!(
        first.insert_or_fetch_conversation_with_quota(&conversation, Some(&job), Some(1)),
        second.insert_or_fetch_conversation_with_quota(&duplicate, Some(&duplicate_job), Some(1))
    );
    let (accepted, accepted_job) = a.unwrap();
    let (replayed, replayed_job) = b.unwrap();
    assert_eq!(accepted.id, replayed.id);
    let accepted_job = accepted_job.unwrap();
    assert_eq!(accepted_job.id, replayed_job.unwrap().id);
    ItemRepository::save(&first, &Item::new_knowledge("quota", "one item"))
        .await
        .unwrap();
    let (replayed, replayed_job) = second
        .insert_or_fetch_conversation_with_quota(&duplicate, Some(&duplicate_job), Some(1))
        .await
        .unwrap();
    assert_eq!(replayed.id, accepted.id);
    assert_eq!(replayed_job.unwrap().id, accepted_job.id);
    assert!(matches!(replayed.status, ConversationStatus::Queued));

    for field in ["owner", "content", "url", "source", "title", "metadata"] {
        let mut conflict = duplicate.clone();
        match field {
            "owner" => conflict.user_id = "someone else".into(),
            "content" => conflict.raw_content = "changed".into(),
            "url" => conflict.url = "https://elsewhere.test".into(),
            "source" => conflict.source = "another provider".into(),
            "title" => conflict.title = Some("changed".into()),
            "metadata" => conflict.metadata = json!({"changed": true}),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                second
                    .insert_or_fetch_conversation_with_quota(&conflict, None, Some(1))
                    .await,
                Err(InfraError::IdempotencyConflict)
            ),
            "field={field}"
        );
    }
    let mut new_request = duplicate.clone();
    new_request.idempotency_key = "new-key".into();
    assert!(matches!(
        second
            .insert_or_fetch_conversation_with_quota(&new_request, Some(&duplicate_job), Some(1))
            .await,
        Err(InfraError::CaptureQuotaExceeded { used: 1, limit: 1 })
    ));
    assert_eq!(first.count_conversations(None).await.unwrap(), 1);
    let conn = Connection::open(path).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM extraction_jobs", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
