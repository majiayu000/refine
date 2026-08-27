use chrono::{TimeZone, Utc};
use refine_core::infra::{configure_sqlite_connection, prepare_sqlite_db, SqliteStore};
use refine_core::knowledge::{DocumentId, Item, ItemId, ItemRepository, ItemType, RestoreParams};
use rusqlite::Connection;
use std::path::Path;

#[tokio::test]
async fn existing_detached_observation_edit_preserves_rowid_link_and_fts() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("detached-edit.db");
    initialize_upsert_fixture(&path);

    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "DROP TRIGGER observations_require_document_insert;
         INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES
           ('legacy-detached', 'observation', 'oldneedle', 'old summary',
            'old content', '[]', NULL, '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z', NULL, NULL);",
    )
    .unwrap();
    let rowid_before: i64 = conn
        .query_row(
            "SELECT rowid FROM items WHERE id = 'legacy-detached'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    prepare_sqlite_db(&conn).unwrap();
    drop(conn);

    let store = SqliteStore::open(&path).unwrap();
    let updated = restored_upsert_item(
        "legacy-detached",
        ItemType::Observation,
        "freshneedle",
        "fresh content",
        None,
    );
    store
        .save(&updated)
        .await
        .expect("editing a historical detached observation must use update semantics");

    let saved = store
        .find_by_id(&ItemId::from("legacy-detached"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.title(), "freshneedle");
    assert_eq!(saved.content(), "fresh content");
    assert!(saved.document_id().is_none());
    assert_eq!(store.count_text_hits("freshneedle").await.unwrap(), 1);
    assert_eq!(store.count_text_hits("oldneedle").await.unwrap(), 0);
    drop(store);

    let conn = Connection::open(&path).unwrap();
    let rowid_after: i64 = conn
        .query_row(
            "SELECT rowid FROM items WHERE id = 'legacy-detached'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rowid_after, rowid_before);
}

#[tokio::test]
async fn new_detached_observation_insert_remains_rejected() {
    let store = SqliteStore::in_memory().unwrap();
    let detached = restored_upsert_item(
        "new-detached",
        ItemType::Observation,
        "new detached",
        "content",
        None,
    );

    let error = store.save(&detached).await.unwrap_err();
    assert!(error
        .to_string()
        .contains("observation requires document_id"));
    assert!(store
        .find_by_id(&ItemId::from("new-detached"))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn linked_observation_cannot_be_updated_to_detached() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("linked-detach.db");
    initialize_upsert_fixture(&path);
    let conn = Connection::open(&path).unwrap();
    insert_upsert_document(&conn, "linked-doc");
    drop(conn);

    let store = SqliteStore::open(&path).unwrap();
    let linked = restored_upsert_item(
        "linked-observation",
        ItemType::Observation,
        "linked title",
        "linked content",
        Some("linked-doc"),
    );
    store.save(&linked).await.unwrap();

    let detached = restored_upsert_item(
        "linked-observation",
        ItemType::Observation,
        "must not persist",
        "must not persist",
        None,
    );
    let error = store.save(&detached).await.unwrap_err();
    assert!(error
        .to_string()
        .contains("observation requires document_id"));

    let unchanged = store
        .find_by_id(&ItemId::from("linked-observation"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.title(), "linked title");
    assert_eq!(
        unchanged.document_id().map(DocumentId::as_str),
        Some("linked-doc")
    );
}

#[tokio::test]
async fn regular_item_save_still_updates_in_place() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("regular-upsert.db");
    initialize_upsert_fixture(&path);

    let store = SqliteStore::open(&path).unwrap();
    let original = restored_upsert_item(
        "knowledge-item",
        ItemType::Knowledge,
        "ordinary old",
        "old body",
        None,
    );
    store.save(&original).await.unwrap();
    drop(store);
    let conn = Connection::open(&path).unwrap();
    let rowid_before: i64 = conn
        .query_row(
            "SELECT rowid FROM items WHERE id = 'knowledge-item'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    conn.execute(
        "INSERT INTO conversations
           (id, user_id, source, url, title, raw_content, metadata_json,
            captured_at, created_at, status, idempotency_key, item_ids, last_error)
         VALUES
           ('conversation', 'user', 'fixture', 'fixture://conversation', NULL,
            'raw', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
            'processed', 'fixture-key', '[\"knowledge-item\"]', NULL)",
        [],
    )
    .unwrap();
    drop(conn);

    let store = SqliteStore::open(&path).unwrap();
    let updated = restored_upsert_item(
        "knowledge-item",
        ItemType::Knowledge,
        "ordinary fresh",
        "fresh body",
        None,
    );
    store.save(&updated).await.unwrap();
    assert_eq!(store.count_text_hits("ordinary fresh").await.unwrap(), 1);
    drop(store);

    let conn = Connection::open(&path).unwrap();
    let (rowid_after, title, count, item_ids): (i64, String, i64, String) = conn
        .query_row(
            "SELECT rowid, title, (SELECT COUNT(*) FROM items),
                    (SELECT item_ids FROM conversations WHERE id = 'conversation')
             FROM items
             WHERE id = 'knowledge-item'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(rowid_after, rowid_before);
    assert_eq!(title, "ordinary fresh");
    assert_eq!(count, 1);
    assert_eq!(item_ids, "[\"knowledge-item\"]");
}

fn initialize_upsert_fixture(path: &Path) {
    let conn = Connection::open(path).unwrap();
    configure_sqlite_connection(&conn).unwrap();
    prepare_sqlite_db(&conn).unwrap();
}

fn insert_upsert_document(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO documents
           (id, title, raw_content, source, url, captured_at, created_at, updated_at)
         VALUES (?1, 'fixture', 'raw', 'fixture', 'fixture://document',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
                 '2026-01-01T00:00:00Z')",
        [id],
    )
    .unwrap();
}

fn restored_upsert_item(
    id: &str,
    item_type: ItemType,
    title: &str,
    content: &str,
    document_id: Option<&str>,
) -> Item {
    let timestamp = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().unwrap();
    Item::restore(RestoreParams {
        id: ItemId::from(id),
        item_type,
        title: title.into(),
        summary: title.into(),
        content: content.into(),
        tags: vec![],
        source: None,
        document_id: document_id.map(DocumentId::from),
        excerpt: None,
        created_at: timestamp,
        updated_at: timestamp,
    })
    .unwrap()
}
