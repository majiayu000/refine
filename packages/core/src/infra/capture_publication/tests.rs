use crate::conversation::{
    ConversationRecord, ConversationRepository, ConversationStatus, ExtractionJobRecord,
    ExtractionMode, JobPublicationOutcome, JobRepository, JobStatus,
};
use crate::infra::SqliteStore;
use crate::knowledge::{DocumentRepository, Item, ItemRepository};
use serde_json::json;

use super::*;

const NOW: &str = "2026-10-06T10:00:00Z";
const EXPIRES: &str = "2099-01-01T00:00:00Z";

fn capture(key: &str, content: &str) -> ConversationRecord {
    ConversationRecord {
        id: format!("conversation-{key}"),
        user_id: "test-user".into(),
        source: "chatgpt".into(),
        url: "https://chatgpt.com/c/shared".into(),
        title: Some(key.into()),
        raw_content: content.into(),
        metadata: json!({}),
        captured_at: NOW.into(),
        created_at: NOW.into(),
        status: ConversationStatus::Queued,
        idempotency_key: key.into(),
        item_ids: vec![],
        last_error: None,
        superseded_by: None,
    }
}

async fn enqueue(store: &SqliteStore, capture: &ConversationRecord) -> ExtractionJobRecord {
    let job = ExtractionJobRecord {
        id: format!("job-{}", capture.id),
        conversation_id: capture.id.clone(),
        mode: ExtractionMode::Auto,
        status: JobStatus::Pending,
        created_at: NOW.into(),
        updated_at: NOW.into(),
        error: None,
        attempt_count: 0,
        lease_owner: None,
        lease_expires_at: None,
    };
    store
        .insert_or_fetch_conversation_with_job(capture, &job)
        .await
        .unwrap()
        .1
        .unwrap()
}

async fn claim(store: &SqliteStore, job: &ExtractionJobRecord) {
    assert!(store
        .claim_job(&job.id, &job.id, NOW, EXPIRES)
        .await
        .unwrap()
        .is_some());
}

fn result(capture: &ConversationRecord) -> (Document, Vec<Item>) {
    let mut doc = Document::new(&capture.source, &capture.raw_content);
    doc.set_url(&capture.url);
    if let Some(title) = &capture.title {
        doc.set_title(title);
    }
    doc.set_captured_at(
        DateTime::parse_from_rfc3339(&capture.captured_at)
            .unwrap()
            .with_timezone(&Utc),
    );
    let mut item = Item::new_knowledge(capture.title.as_deref().unwrap(), &capture.raw_content);
    item.set_document_id(doc.id().clone());
    (doc, vec![item])
}

async fn publish(
    store: &SqliteStore,
    job: &ExtractionJobRecord,
    capture: &ConversationRecord,
) -> InfraResult<JobPublicationOutcome> {
    let (doc, items) = result(capture);
    store
        .finish_job_claim_with_results(&job.id, &job.id, &doc, &items, NOW)
        .await
}

#[tokio::test]
async fn newer_publication_survives_older_completion_and_superseded_receipt_is_durable() {
    let store = SqliteStore::in_memory().unwrap();
    let mut old = capture("old", "Use MySQL");
    old.captured_at = "2090-01-01T00:00:00Z".into();
    let old_job = enqueue(&store, &old).await;
    let mut new = capture("new", "Use PostgreSQL");
    new.captured_at = "2000-01-01T00:00:00Z".into();
    let new_job = enqueue(&store, &new).await;
    claim(&store, &old_job).await;
    claim(&store, &new_job).await;
    assert_eq!(
        publish(&store, &new_job, &new).await.unwrap(),
        JobPublicationOutcome::Published
    );
    let before = store.find_by_url(&new.url).await.unwrap().unwrap();
    let items_before = store.find_by_document_id(before.id()).await.unwrap();

    assert_eq!(
        publish(&store, &old_job, &old).await.unwrap(),
        JobPublicationOutcome::Superseded
    );
    let after = store.find_by_url(&new.url).await.unwrap().unwrap();
    assert_eq!(after.raw_content(), "Use PostgreSQL");
    assert_eq!(after.id(), before.id());
    let items_after = store.find_by_document_id(after.id()).await.unwrap();
    assert_eq!(items_after.len(), 1);
    assert_eq!(items_after[0].id(), items_before[0].id());
    let receipt = store.find_job_by_id(&old_job.id).await.unwrap().unwrap();
    assert_eq!(receipt.status, JobStatus::Succeeded);
    assert!(receipt.error.is_none());
    let conversation = store
        .find_conversation_by_id(&old.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(conversation.status, ConversationStatus::Processed);
    assert_eq!(conversation.superseded_by.as_deref(), Some(new.id.as_str()));
    assert!(conversation.item_ids.is_empty());
    assert_eq!(conversation.last_error, receipt.error);
    assert!(store
        .list_recoverable_jobs(EXPIRES, 10)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn newer_pending_or_failed_capture_does_not_block_older_publication() {
    for newer_fails in [false, true] {
        let store = SqliteStore::in_memory().unwrap();
        let old = capture("old", "First valid result");
        let old_job = enqueue(&store, &old).await;
        let new = capture("new", "Later valid result");
        let new_job = enqueue(&store, &new).await;
        claim(&store, &old_job).await;
        if newer_fails {
            claim(&store, &new_job).await;
            assert!(store
                .finish_job_claim(
                    &new_job.id,
                    &new_job.id,
                    JobStatus::Failed,
                    &[],
                    Some("provider failed"),
                    NOW
                )
                .await
                .unwrap());
        }
        assert_eq!(
            publish(&store, &old_job, &old).await.unwrap(),
            JobPublicationOutcome::Published
        );
        assert_eq!(
            store
                .find_by_url(&old.url)
                .await
                .unwrap()
                .unwrap()
                .raw_content(),
            old.raw_content
        );
        if !newer_fails {
            claim(&store, &new_job).await;
            assert_eq!(
                publish(&store, &new_job, &new).await.unwrap(),
                JobPublicationOutcome::Published
            );
            assert_eq!(
                store
                    .find_by_url(&new.url)
                    .await
                    .unwrap()
                    .unwrap()
                    .raw_content(),
                new.raw_content
            );
        }
    }
}

#[tokio::test]
async fn source_change_and_revert_after_claim_cannot_reuse_the_old_revision() {
    let store = SqliteStore::in_memory().unwrap();
    let original = capture("source", "Original passage");
    let job = enqueue(&store, &original).await;
    claim(&store, &job).await;
    let mut changed = store
        .find_conversation_by_id(&original.id)
        .await
        .unwrap()
        .unwrap();
    changed.raw_content = "Different passage".into();
    store.upsert_conversation(&changed).await.unwrap();
    changed.raw_content = original.raw_content.clone();
    store.upsert_conversation(&changed).await.unwrap();
    let error = publish(&store, &job, &original)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("capture_source_changed"), "{error}");
    assert!(store.find_by_url(&original.url).await.unwrap().is_none());
    assert_eq!(store.count_items(None).await.unwrap(), 0);
}

#[tokio::test]
async fn quota_conflict_and_receipt_replay_preserve_receive_revision_and_initial_input() {
    let store = SqliteStore::in_memory().unwrap();
    let original = capture("same-key", "Original passage");
    let job = enqueue(&store, &original).await;
    claim(&store, &job).await;
    let mut replay = original.clone();
    replay.id = "different-proposed-id".into();
    replay.raw_content = "Retry cannot replace accepted input".into();
    assert!(matches!(
        store
            .insert_or_fetch_conversation_with_quota(&replay, None, None)
            .await,
        Err(InfraError::IdempotencyConflict)
    ));
    // The legacy receipt lookup also returns the stored payload without writing
    // a new source revision; strict HTTP admission uses the quota API above.
    let existing = store
        .insert_or_fetch_conversation_by_idempotency(&replay)
        .await
        .unwrap();
    assert_eq!(existing.id, original.id);
    assert_eq!(existing.raw_content, original.raw_content);
    assert_eq!(
        publish(&store, &job, &original).await.unwrap(),
        JobPublicationOutcome::Published
    );
}

#[tokio::test]
async fn failed_result_transaction_does_not_advance_publication_revision() {
    let store = SqliteStore::in_memory().unwrap();
    let old = capture("old", "Older valid source");
    let old_job = enqueue(&store, &old).await;
    let new = capture("new", "Newer result fails persistence");
    let new_job = enqueue(&store, &new).await;
    claim(&store, &old_job).await;
    claim(&store, &new_job).await;
    let (doc, mut items) = result(&new);
    items[0].set_document_id(Document::new("missing", "missing").id().clone());
    assert!(store
        .finish_job_claim_with_results(&new_job.id, &new_job.id, &doc, &items, NOW)
        .await
        .is_err());
    assert!(store.find_by_url(&new.url).await.unwrap().is_none());
    assert_eq!(
        publish(&store, &old_job, &old).await.unwrap(),
        JobPublicationOutcome::Published
    );
}

#[test]
fn migration_preserves_unknown_history_without_inventing_receive_order() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../schema.sql")).unwrap();
    conn.execute(
        "INSERT INTO conversations(id,user_id,source,url,raw_content,metadata_json,captured_at,created_at,status,idempotency_key,item_ids)
         VALUES('legacy','user','chatgpt','https://chatgpt.com/c/shared','Legacy content','{}',?1,?1,'processing','legacy-key','[]')",
        [NOW],
    ).unwrap();
    conn.execute("INSERT INTO extraction_jobs(id,conversation_id,mode,status,created_at,updated_at,source_revision,lease_owner)
        VALUES('legacy-job','legacy','auto','running',?1,?1,0,'worker')", [NOW]).unwrap();
    prepare(&conn).unwrap();
    prepare(&conn).unwrap();
    let legacy: i64 = conn
        .query_row(
            "SELECT COALESCE((SELECT revision FROM capture_revisions WHERE conversation_id='legacy'),0)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(legacy, 0);
    let mut legacy_doc = Document::new("chatgpt", "Legacy content");
    legacy_doc.set_url("https://chatgpt.com/c/shared");
    legacy_doc.set_captured_at(
        DateTime::parse_from_rfc3339(NOW)
            .unwrap()
            .with_timezone(&Utc),
    );
    let legacy_source = claimed_source(&conn, "legacy-job", "worker", &legacy_doc)
        .unwrap()
        .unwrap();
    assert_eq!(legacy_source.revision, 0);
    conn.execute(
        "INSERT INTO conversations(id,user_id,source,url,raw_content,metadata_json,captured_at,created_at,status,idempotency_key,item_ids)
         VALUES('new','user','chatgpt','https://chatgpt.com/c/shared','New content','{}',?1,?1,'queued','new-key','[]')",
        [NOW],
    ).unwrap();
    let current: i64 = conn
        .query_row(
            "SELECT revision FROM capture_revisions WHERE conversation_id='new'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(current > 0);
    let clock: i64 = conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='capture_revisions'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    prepare(&conn).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='capture_revisions'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        clock
    );
}

#[tokio::test]
async fn legacy_zero_can_publish_until_a_new_revision_has_successfully_published() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite");
    let legacy = [
        capture("legacy-a", "First legacy result"),
        capture("legacy-b", "Second legacy result"),
        capture("legacy-c", "Late legacy result"),
    ];
    {
        let conn = Connection::open(&path).unwrap();
        // This is the pre-revision schema: migration must add the nullable
        // claim column without assigning fabricated positive source order.
        let old_schema =
            include_str!("../schema.sql").replace("    source_revision INTEGER,\n", "");
        conn.execute_batch(&old_schema).unwrap();
        for capture in &legacy {
            conn.execute(
                "INSERT INTO conversations(id,user_id,source,url,title,raw_content,metadata_json,captured_at,created_at,status,idempotency_key,item_ids)
                 VALUES(?1,'test-user',?2,?3,?4,?5,'{}',?6,?6,'queued',?7,'[]')",
                params![capture.id, capture.source, capture.url, capture.title, capture.raw_content, NOW, capture.idempotency_key],
            ).unwrap();
        }
    }
    let store = SqliteStore::open(&path).unwrap();
    let mut jobs = Vec::new();
    for capture in &legacy {
        jobs.push(enqueue(&store, capture).await);
    }
    let new = capture("new", "New revision result");
    let new_job = enqueue(&store, &new).await;
    // Admission of a positive revision does not fence out unfinished history.
    for index in 0..2 {
        claim(&store, &jobs[index]).await;
        assert_eq!(
            publish(&store, &jobs[index], &legacy[index]).await.unwrap(),
            JobPublicationOutcome::Published
        );
    }
    claim(&store, &new_job).await;
    assert_eq!(
        publish(&store, &new_job, &new).await.unwrap(),
        JobPublicationOutcome::Published
    );
    claim(&store, &jobs[2]).await;
    assert_eq!(
        publish(&store, &jobs[2], &legacy[2]).await.unwrap(),
        JobPublicationOutcome::Superseded
    );
    assert_eq!(
        store
            .find_by_url(&new.url)
            .await
            .unwrap()
            .unwrap()
            .raw_content(),
        new.raw_content
    );
}

#[tokio::test]
async fn legacy_database_import_keeps_unknown_revisions_and_preserves_local_publication() {
    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("refine.db");
    let target = SqliteStore::open(&target_path).unwrap();
    let local = capture("local", "Current local result");
    let local_job = enqueue(&target, &local).await;
    claim(&target, &local_job).await;
    assert_eq!(
        publish(&target, &local_job, &local).await.unwrap(),
        JobPublicationOutcome::Published
    );

    let legacy_path = directory.path().join("server.db");
    let legacy = SqliteStore::open(&legacy_path).unwrap();
    let mut foreign = capture("foreign", "Foreign clock cannot establish freshness");
    foreign.captured_at = "2099-01-01T00:00:00Z".into();
    let foreign_job = enqueue(&legacy, &foreign).await;
    claim(&legacy, &foreign_job).await;
    assert_eq!(
        publish(&legacy, &foreign_job, &foreign).await.unwrap(),
        JobPublicationOutcome::Published
    );

    crate::infra::migrate_stale_dbs(&target_path).unwrap();
    let conn = Connection::open(&target_path).unwrap();
    let imported_revision: i64 = conn
        .query_row(
            "SELECT COALESCE((SELECT revision FROM capture_revisions WHERE conversation_id=?1),0)",
            [&foreign.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(imported_revision, 0);
    let imported_claim: Option<i64> = conn
        .query_row(
            "SELECT source_revision FROM extraction_jobs WHERE id=?1",
            [&foreign_job.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(imported_claim, None);
    let clock: (i64, bool) = conn
        .query_row(
            "SELECT (SELECT seq FROM sqlite_sequence WHERE name='capture_revisions'), legacy_import FROM capture_publication_context",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(clock, (1, false));
    let doc = target.find_by_url(&local.url).await.unwrap().unwrap();
    assert_eq!(doc.raw_content(), local.raw_content);
    let items = target.find_by_document_id(doc.id()).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title(), "local");

    let fresh = capture("fresh", "Fresh admission after import");
    let fresh_job = enqueue(&target, &fresh).await;
    claim(&target, &fresh_job).await;
    assert_eq!(
        publish(&target, &fresh_job, &fresh).await.unwrap(),
        JobPublicationOutcome::Published
    );
    let revision: i64 = conn
        .query_row(
            "SELECT COALESCE((SELECT revision FROM capture_revisions WHERE conversation_id=?1),0)",
            [&fresh.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision, 2);
}

#[tokio::test]
async fn legacy_import_cannot_overwrite_published_rows_by_reusing_their_ids() {
    for has_document_column in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let target_path = directory.path().join("refine.db");
        let target = SqliteStore::open(&target_path).unwrap();
        let local = capture("local", "Current local source");
        let job = enqueue(&target, &local).await;
        claim(&target, &job).await;
        assert_eq!(
            publish(&target, &job, &local).await.unwrap(),
            JobPublicationOutcome::Published
        );
        let doc = target.find_by_url(&local.url).await.unwrap().unwrap();
        let item = target
            .find_by_document_id(doc.id())
            .await
            .unwrap()
            .remove(0);

        let legacy_path = directory.path().join("server.db");
        let conn = Connection::open(&legacy_path).unwrap();
        let document_column = if has_document_column {
            "document_id TEXT,"
        } else {
            ""
        };
        conn.execute_batch(&format!(
            "CREATE TABLE documents (
                 id TEXT PRIMARY KEY, title TEXT, raw_content TEXT NOT NULL,
                 source TEXT NOT NULL, url TEXT NOT NULL, captured_at TEXT NOT NULL,
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );
             CREATE TABLE items (
                 id TEXT PRIMARY KEY, item_type TEXT NOT NULL, title TEXT NOT NULL,
                 summary TEXT NOT NULL, content TEXT NOT NULL, tags TEXT NOT NULL,
                 {document_column}
                 created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );"
        ))
        .unwrap();
        // The ID conflicts with the published document but the incoming URL
        // differs, so checking only src.url would miss this overwrite.
        conn.execute(
            "INSERT INTO documents VALUES(?1,'Legacy title','Legacy replacement','chatgpt',
             'https://chatgpt.com/c/changed-url','2099-01-01T00:00:00Z',
             '2099-01-01T00:00:00Z','2099-01-01T00:00:00Z')",
            [doc.id().as_str()],
        )
        .unwrap();
        // Either no parent column exists, or the incoming parent is NULL.
        // Neither may bypass protection of the existing same-ID item's parent.
        conn.execute(
            "INSERT INTO items(id,item_type,title,summary,content,tags,created_at,updated_at)
             VALUES(?1,'knowledge','Legacy replacement','Legacy','Legacy payload','[]',
             '2099-01-01T00:00:00Z','2099-01-01T00:00:00Z')",
            [item.id().as_str()],
        )
        .unwrap();
        drop(conn);

        crate::infra::migrate_stale_dbs(&target_path).unwrap();
        let after = target.find_by_url(&local.url).await.unwrap().unwrap();
        assert_eq!(after.id(), doc.id());
        assert_eq!(after.raw_content(), local.raw_content);
        let after_item = ItemRepository::find_by_id(&target, item.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_item.document_id(), Some(doc.id()));
        assert_eq!(after_item.title(), item.title());
        assert_eq!(after_item.content(), item.content());
    }
}

#[tokio::test]
async fn legacy_import_preserves_accepted_capture_and_its_running_claim() {
    for case in ["same-key", "same-id", "job-reparent", "extra-job"] {
        let directory = tempfile::tempdir().unwrap();
        let target_path = directory.path().join("refine.db");
        let target = SqliteStore::open(&target_path).unwrap();
        let local = capture("local", "Accepted local input");
        let local_job = enqueue(&target, &local).await;
        claim(&target, &local_job).await;

        let legacy_path = directory.path().join("server.db");
        let legacy = SqliteStore::open(&legacy_path).unwrap();
        let mut foreign = capture(
            "foreign",
            "Imported input must not replace the active source",
        );
        foreign.user_id = "foreign-owner".into();
        match case {
            "same-key" => foreign.idempotency_key = local.idempotency_key.clone(),
            "same-id" | "extra-job" => foreign.id = local.id.clone(),
            "job-reparent" => {}
            _ => unreachable!(),
        }
        let foreign_job = ExtractionJobRecord {
            id: if case == "job-reparent" || case == "same-id" {
                local_job.id.clone()
            } else {
                "foreign-extra-job".into()
            },
            conversation_id: foreign.id.clone(),
            ..local_job.clone()
        };
        legacy
            .insert_or_fetch_conversation_with_job(&foreign, &foreign_job)
            .await
            .unwrap();
        legacy
            .claim_job(
                &foreign_job.id,
                "foreign-worker",
                "2090-01-01T00:00:00Z",
                EXPIRES,
            )
            .await
            .unwrap()
            .unwrap();

        crate::infra::migrate_stale_dbs(&target_path).unwrap();
        let after = target
            .find_conversation_by_id(&local.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.raw_content, local.raw_content, "{case}");
        assert_eq!(after.user_id, local.user_id, "{case}");
        assert_eq!(after.idempotency_key, local.idempotency_key, "{case}");
        let job = target.find_job_by_id(&local_job.id).await.unwrap().unwrap();
        assert_eq!(job.conversation_id, local.id, "{case}");
        assert_eq!(job.status, JobStatus::Running, "{case}");
        assert_eq!(
            job.lease_owner.as_deref(),
            Some(local_job.id.as_str()),
            "{case}"
        );
        let conn = Connection::open(&target_path).unwrap();
        let revisions: (i64, Option<i64>) = conn
            .query_row(
                "SELECT r.revision,j.source_revision FROM capture_revisions r
             JOIN extraction_jobs j ON j.conversation_id=r.conversation_id WHERE j.id=?1",
                [&local_job.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(revisions, (1, Some(1)), "{case}");
        if foreign_job.id != local_job.id {
            assert!(
                target
                    .find_job_by_id(&foreign_job.id)
                    .await
                    .unwrap()
                    .is_none(),
                "{case}"
            );
        }
        assert_eq!(
            publish(&target, &local_job, &local).await.unwrap(),
            JobPublicationOutcome::Published,
            "{case}"
        );
    }
}
