use refine_core::infra::SqliteStore;
use refine_core::knowledge::ItemRepository;
use rusqlite::Connection;

#[tokio::test]
async fn opening_legacy_db_rebuilds_fts_index_for_existing_rows() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = dir.path().join("legacy.db");

    let conn = Connection::open(&db_path).expect("failed to create legacy db");
    conn.execute_batch(
        r#"
        CREATE TABLE items (
            id TEXT PRIMARY KEY,
            item_type TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            content TEXT NOT NULL,
            tags TEXT NOT NULL,
            source TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        INSERT INTO items (id, item_type, title, summary, content, tags, source, created_at, updated_at)
        VALUES (
            'legacy-1',
            'knowledge',
            'Legacy Rust',
            'legacy summary',
            'legacy rust content',
            '["rust"]',
            NULL,
            '2026-02-01T00:00:00Z',
            '2026-02-01T00:00:00Z'
        );
        "#,
    )
    .expect("failed to prepare legacy rows");
    drop(conn);

    let store = SqliteStore::open(&db_path).expect("failed to open sqlite store");
    let total = store
        .count_text_hits("rust")
        .await
        .expect("count_text_hits failed");
    assert_eq!(total, 1);

    let rows = store
        .search_text("rust", 0, 10)
        .await
        .expect("search_text failed");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id().as_str(), "legacy-1");
}
