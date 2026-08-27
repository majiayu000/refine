use super::super::{migrate_stale_dbs, MigrationReport};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[test]
fn imports_observation_from_schema_without_document_id_then_restores_guard() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("refine.db");
    let legacy = create_legacy_items(temp.path(), false);
    insert_detached_observation(&legacy, false);

    assert_migrated_detached_observation(&target);
}

#[test]
fn imports_existing_null_observation_then_restores_guard() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("refine.db");
    let legacy = create_legacy_items(temp.path(), true);
    insert_detached_observation(&legacy, true);

    assert_migrated_detached_observation(&target);
}

#[test]
fn failed_import_rolls_back_rows_and_trigger_suspension() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("refine.db");
    let legacy = create_legacy_items(temp.path(), false);
    insert_detached_observation(&legacy, false);
    let conn = Connection::open(&legacy).unwrap();
    conn.execute_batch(
        "CREATE TABLE extraction_jobs (
           id TEXT PRIMARY KEY,
           conversation_id TEXT NOT NULL,
           mode TEXT NOT NULL,
           status TEXT NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           error TEXT
         );
         INSERT INTO extraction_jobs
           (id, conversation_id, mode, status, created_at, updated_at, error)
         VALUES
           ('orphan-job', 'missing-conversation', 'auto', 'pending',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL);",
    )
    .unwrap();
    drop(conn);

    assert!(migrate_stale_dbs(&target).is_err());
    let conn = Connection::open(&target).unwrap();
    let imported: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items WHERE id = 'legacy-observation'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        imported, 0,
        "item copy must roll back with the later failure"
    );
    assert_new_detached_is_rejected(&conn);
}

fn create_legacy_items(directory: &Path, with_document_id: bool) -> PathBuf {
    let path = directory.join("server.db");
    let conn = Connection::open(&path).unwrap();
    let document_column = if with_document_id {
        ", document_id TEXT"
    } else {
        ""
    };
    conn.execute_batch(&format!(
        "CREATE TABLE items (
           id TEXT PRIMARY KEY,
           item_type TEXT NOT NULL,
           title TEXT NOT NULL,
           summary TEXT NOT NULL,
           content TEXT NOT NULL,
           tags TEXT NOT NULL,
           source TEXT,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL
           {document_column}
         );"
    ))
    .unwrap();
    path
}

fn insert_detached_observation(path: &Path, with_document_id: bool) {
    let conn = Connection::open(path).unwrap();
    let document_column = if with_document_id {
        ", document_id"
    } else {
        ""
    };
    let document_value = if with_document_id { ", NULL" } else { "" };
    conn.execute_batch(&format!(
        "INSERT INTO items
           (id, item_type, title, summary, content, tags, source, created_at, updated_at{document_column})
         VALUES
           ('legacy-observation', 'observation', 'Legacy', 'Legacy', '', '[]',
            'legacy', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'{document_value});"
    ))
    .unwrap();
}

fn assert_migrated_detached_observation(target: &Path) {
    let report = migrate_stale_dbs(target).unwrap();
    assert!(matches!(
        report,
        MigrationReport::Migrated { rows_copied: 1, .. }
    ));

    let conn = Connection::open(target).unwrap();
    let detached: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items
             WHERE id = 'legacy-observation'
               AND item_type = 'observation'
               AND document_id IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(detached, 1, "historical detached row must be preserved");

    assert_new_detached_is_rejected(&conn);
}

fn assert_new_detached_is_rejected(conn: &Connection) {
    let error = conn
        .execute(
            "INSERT INTO items
               (id, item_type, title, summary, content, tags, source,
                created_at, updated_at, document_id, excerpt)
             VALUES
               ('new-detached', 'observation', 'New', 'New', '', '[]', 'test',
                '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', NULL, NULL)",
            [],
        )
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("observation requires document_id"));
}
