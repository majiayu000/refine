use std::time::Duration;

use crate::infra::{configure_sqlite_connection, prepare_sqlite_db};
use crate::knowledge::Item;

use super::*;

fn capture(id: &str, key: &str) -> ConversationRecord {
    ConversationRecord {
        id: id.into(),
        user_id: "user".into(),
        source: "chatgpt".into(),
        url: "https://chatgpt.com/c/quota-race".into(),
        title: None,
        raw_content: "User: original capture".into(),
        metadata: serde_json::json!({}),
        captured_at: "2026-10-06T00:00:00Z".into(),
        created_at: "2026-10-06T00:00:00Z".into(),
        status: ConversationStatus::Captured,
        idempotency_key: key.into(),
        item_ids: vec![],
        last_error: None,
        superseded_by: None,
    }
}

fn initial_job(parent: &str) -> ExtractionJobRecord {
    ExtractionJobRecord {
        id: "new-job".into(),
        conversation_id: parent.into(),
        mode: ExtractionMode::Auto,
        status: JobStatus::Pending,
        created_at: "2026-10-06T00:00:00Z".into(),
        updated_at: "2026-10-06T00:00:00Z".into(),
        error: None,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
    }
}

#[test]
fn quota_admission_rechecks_key_after_a_concurrent_writer_removes_it() {
    for mutation in [
        "UPDATE conversations SET idempotency_key='retired-key' WHERE id='old'",
        "DELETE FROM conversations WHERE id='old'",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.sqlite");
        let mut writer = Connection::open(&path).unwrap();
        configure_sqlite_connection(&writer).unwrap();
        prepare_sqlite_db(&writer).unwrap();
        insert_or_fetch_conversation_by_idempotency(&writer, &capture("old", "retry-key")).unwrap();
        super::super::ops::save(&writer, &Item::new_knowledge("Existing", "Quota is full"))
            .unwrap();

        let reader = Connection::open(&path).unwrap();
        configure_sqlite_connection(&reader).unwrap();
        reader.busy_timeout(Duration::ZERO).unwrap();
        let tx = writer
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute(mutation, []).unwrap();

        // WAL readers still see the old key until the writer commits. Such a
        // read cannot authorize an INSERT after the key is deleted or rekeyed.
        assert_eq!(
            find_conversation_by_idempotency(&reader, "retry-key")
                .unwrap()
                .unwrap()
                .id,
            "old"
        );
        let retry = capture("retry", "retry-key");
        let job = initial_job(&retry.id);
        assert!(matches!(
            insert_or_fetch_conversation_with_quota(&reader, &retry, Some(&job), Some(1)),
            Err(InfraError::Database(_))
        ));
        tx.commit().unwrap();

        assert!(matches!(
            insert_or_fetch_conversation_with_quota(&reader, &retry, Some(&job), Some(1)),
            Err(InfraError::CaptureQuotaExceeded { used: 1, limit: 1 })
        ));
        assert!(find_conversation_by_id(&reader, "retry").unwrap().is_none());
        assert!(find_job_by_id(&reader, "new-job").unwrap().is_none());
    }
}

#[test]
fn quota_admission_rolls_back_a_new_capture_if_its_job_cannot_be_written() {
    let conn = Connection::open_in_memory().unwrap();
    configure_sqlite_connection(&conn).unwrap();
    prepare_sqlite_db(&conn).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_job BEFORE INSERT ON extraction_jobs BEGIN
             SELECT RAISE(ABORT, 'synthetic job write failure');
         END;",
    )
    .unwrap();
    let conversation = capture("new", "new-key");
    let job = initial_job(&conversation.id);
    let error = insert_or_fetch_conversation_with_quota(&conn, &conversation, Some(&job), Some(1))
        .unwrap_err();
    assert!(error.to_string().contains("synthetic job write failure"));
    assert!(find_conversation_by_idempotency(&conn, "new-key")
        .unwrap()
        .is_none());
    assert!(find_job_by_id(&conn, "new-job").unwrap().is_none());
}
