use refine_core::infra::SqliteStore;
use rusqlite::Connection;
use std::sync::Arc;

pub(crate) fn legacy_detached_store() -> (tempfile::TempDir, Arc<SqliteStore>) {
    let directory = tempfile::tempdir().expect("create legacy detached fixture directory");
    let db_path = directory.path().join("refine.db");
    let conn = Connection::open(&db_path).expect("open legacy detached fixture");
    conn.execute_batch(
        "CREATE TABLE documents (
           id TEXT PRIMARY KEY, title TEXT, raw_content TEXT NOT NULL,
           source TEXT NOT NULL, url TEXT NOT NULL, captured_at TEXT NOT NULL,
           created_at TEXT NOT NULL, updated_at TEXT NOT NULL
         );
         CREATE TABLE items (
           id TEXT PRIMARY KEY, item_type TEXT NOT NULL, title TEXT NOT NULL,
           summary TEXT NOT NULL, content TEXT NOT NULL, tags TEXT NOT NULL,
           source TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
           document_id TEXT, excerpt TEXT
         );
         INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES
           ('legacy-detached', 'observation', 'detached', 'legacy evidence', '', '[]', NULL,
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL, NULL);",
    )
    .expect("seed detached row before fail-closed triggers exist");
    drop(conn);
    let store = Arc::new(SqliteStore::open(&db_path).expect("upgrade legacy detached fixture"));
    (directory, store)
}
