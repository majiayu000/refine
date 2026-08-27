use chrono::{TimeZone, Utc};
use refine_core::infra::{
    configure_sqlite_connection,
    item_link_repair::{apply_repair, plan_repair},
    prepare_sqlite_db, SqliteStore,
};
use refine_core::knowledge::{
    Document, DocumentId, DocumentRepository, Item, ItemId, ItemRepository, RestoreDocumentParams,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[tokio::test]
async fn repair_ledger_does_not_pin_items_or_documents_during_refresh_and_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let current = temp.path().join("current.db");
    let evidence = temp.path().join("evidence.db");
    seed_lifecycle_current(&current);
    seed_lifecycle_evidence(&evidence);
    let evidence_hash = sha256_file(&evidence);
    let backup = temp.path().join("before-repair.db");

    let plan = plan_repair(&current, &evidence, &evidence_hash).unwrap();
    assert_eq!(plan.stats.candidate_items, 1, "{:#?}", plan.stats);
    let report = apply_repair(&current, &evidence, &evidence_hash, &backup).unwrap();
    assert_eq!(report.changed_items, 1);
    assert_eq!(report.ledger_rows_added, 1);
    let ledger_before = read_ledger(&current);

    let timestamp = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).single().unwrap();
    let document = Document::restore(RestoreDocumentParams {
        id: DocumentId::from("repair-doc"),
        title: Some("Refreshed summary".into()),
        raw_content: "refreshed raw content".into(),
        source: "fixture".into(),
        url: "fixture://repair-doc".into(),
        source_version: None,
        captured_at: timestamp,
        created_at: timestamp,
        updated_at: timestamp,
    });
    let mut replacement = Item::new_observation("replacement", "replacement");
    replacement.set_document_id(document.id().clone());

    let store = SqliteStore::open(&current).unwrap();
    store
        .save_with_replaced_items(&document, &[replacement.clone()])
        .await
        .expect("normal aggregate refresh must replace a repaired item");
    assert!(
        ItemRepository::find_by_id(&store, &ItemId::from("repair-item"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(ItemRepository::find_by_id(&store, replacement.id())
        .await
        .unwrap()
        .is_some());
    drop(store);
    assert_eq!(read_ledger(&current), ledger_before);

    let store = SqliteStore::open(&current).unwrap();
    store
        .delete_documents_with_items(&[document.id().clone()])
        .await
        .expect("explicit aggregate cleanup must not be pinned by audit history");
    drop(store);

    assert_eq!(read_ledger(&current), ledger_before);
    let conn = Connection::open(&current).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM items", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM documents", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert!(conn
        .execute(
            "UPDATE item_link_repair_ledger SET rule_version = 'changed'",
            []
        )
        .unwrap_err()
        .to_string()
        .contains("append-only"));
    assert!(conn
        .execute("DELETE FROM item_link_repair_ledger", [])
        .unwrap_err()
        .to_string()
        .contains("append-only"));
}

fn seed_lifecycle_current(path: &Path) {
    let conn = Connection::open(path).unwrap();
    configure_sqlite_connection(&conn).unwrap();
    prepare_sqlite_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO documents
           (id, title, raw_content, source, url, captured_at, created_at, updated_at)
         VALUES
           ('repair-doc', 'summary', 'raw', 'fixture', 'fixture://repair-doc',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z');
         DROP TRIGGER observations_require_document_insert;
         INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES
           ('repair-item', 'observation', 'summary', 'summary', '', '[]', 'fixture',
            '2026-01-01T00:00:00.500Z', '2026-01-01T00:00:00.500Z', NULL, NULL);",
    )
    .unwrap();
    prepare_sqlite_db(&conn).unwrap();
}

fn seed_lifecycle_evidence(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE documents (id TEXT PRIMARY KEY);
         CREATE TABLE items (
           id TEXT PRIMARY KEY, item_type TEXT NOT NULL, title TEXT NOT NULL,
           created_at TEXT NOT NULL, document_id TEXT
         );
         INSERT INTO items VALUES
           ('repair-item', 'observation', 'summary',
            '2026-01-01T00:00:00.500Z', 'shadow-doc');",
    )
    .unwrap();
}

fn sha256_file(path: &Path) -> String {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).unwrap();
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    format!("{:x}", hasher.finalize())
}

fn read_ledger(path: &Path) -> Vec<(String, String, String, String, String)> {
    let conn = Connection::open(path).unwrap();
    let mut statement = conn
        .prepare(
            "SELECT item_id, target_document_id, evidence_sha256, rule_version, applied_at
             FROM item_link_repair_ledger ORDER BY item_id",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
