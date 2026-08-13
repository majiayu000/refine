//! 基础设施模块
//!
//! ## 文件结构
//!
//! - `sqlite/` - SQLite 存储实现（worker + 查询）
//! - `llm.rs` - LLM 客户端 (Claude, OpenAI)
//! - `schema.sql` - 数据库 Schema

use crate::error::{InfraError, InfraResult};
use rusqlite::Connection;

mod contract;
mod db_migration;
mod llm;
mod llm_retry;
mod paths;
pub mod quota_state;
mod sqlite;

const FTS_BOOTSTRAP_USER_VERSION: i64 = 1;
const ALLOWED_COLUMN_EXISTS_TABLES: &[&str] = &["items", "documents", "extraction_jobs"];
const ALLOWED_FOREIGN_KEY_TABLES: &[&str] = &["items", "extraction_jobs"];

pub fn prepare_sqlite_db(conn: &Connection) -> InfraResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;

    tx.execute_batch(include_str!("schema.sql"))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    migrate_items_add_document_columns(&tx)?;
    migrate_documents_add_source_version(&tx)?;
    migrate_documents_url_unique(&tx)?;
    migrate_items_document_fk(&tx)?;
    migrate_extraction_jobs_add_lease_columns(&tx)?;
    migrate_extraction_jobs_conversation_fk(&tx)?;
    maybe_rebuild_fts_index(&tx)?;
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}

/// Apply the canonical PRAGMA configuration (foreign_keys, WAL, busy_timeout,
/// temp_store, synchronous) to a freshly opened SQLite connection.
///
/// Every entry point that opens a connection to the shared application database
/// must call this — `foreign_keys` and `busy_timeout` are connection-scoped, so
/// missing the call leaves the connection running with SQLite defaults
/// regardless of how the WAL journal was initialised on the file.
pub fn configure_sqlite_connection(conn: &Connection) -> InfraResult<()> {
    let in_memory = matches!(conn.path(), Some(":memory:") | None);
    sqlite::configure_connection(conn, in_memory)
}

pub(crate) fn maybe_rebuild_fts_index(conn: &Connection) -> InfraResult<bool> {
    let user_version = conn
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if user_version < FTS_BOOTSTRAP_USER_VERSION {
        let item_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .map_err(|e| InfraError::Database(e.to_string()))?;
        let rebuilt = if item_count > 0 {
            conn.execute("INSERT INTO items_fts(items_fts) VALUES('rebuild')", [])
                .map_err(|e| InfraError::Database(e.to_string()))?;
            true
        } else {
            false
        };
        conn.pragma_update(None, "user_version", FTS_BOOTSTRAP_USER_VERSION)
            .map_err(|e| InfraError::Database(e.to_string()))?;
        return Ok(rebuilt);
    }

    if conn
        .execute(
            "INSERT INTO items_fts(items_fts) VALUES('integrity-check')",
            [],
        )
        .is_ok()
    {
        return Ok(false);
    }

    conn.execute("INSERT INTO items_fts(items_fts) VALUES('rebuild')", [])
        .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute(
        "INSERT INTO items_fts(items_fts) VALUES('integrity-check')",
        [],
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(true)
}

pub(crate) fn column_exists(conn: &Connection, table: &str, column: &str) -> InfraResult<bool> {
    if !ALLOWED_COLUMN_EXISTS_TABLES.contains(&table) {
        return Err(InfraError::Database(format!("unknown table: {table}")));
    }
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| InfraError::Database(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names.iter().any(|n| n == column))
}

fn migrate_items_add_document_columns(conn: &Connection) -> InfraResult<()> {
    let has_document_id = column_exists(conn, "items", "document_id")?;
    if !has_document_id {
        conn.execute_batch("ALTER TABLE items ADD COLUMN document_id TEXT")
            .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    let has_excerpt = column_exists(conn, "items", "excerpt")?;
    if !has_excerpt {
        conn.execute_batch("ALTER TABLE items ADD COLUMN excerpt TEXT")
            .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_items_document ON items(document_id)")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn migrate_documents_add_source_version(conn: &Connection) -> InfraResult<()> {
    if !column_exists(conn, "documents", "source_version")? {
        conn.execute_batch("ALTER TABLE documents ADD COLUMN source_version TEXT")
            .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    Ok(())
}

fn migrate_documents_url_unique(conn: &Connection) -> InfraResult<()> {
    let unique_index_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM pragma_index_list('documents')
             WHERE name = 'idx_documents_url' AND \"unique\" = 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if unique_index_exists > 0 {
        return Ok(());
    }

    // The legacy schema has no URL index. Build a temporary migration index
    // before running the correlated merge queries so upgrade cost scales with
    // duplicate groups rather than repeatedly scanning the whole table.
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_documents_url_migration ON documents(url)")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    // Preserve the first document identity for compatibility with the normal
    // `ON CONFLICT(url)` save path, but merge in the newest authoritative
    // snapshot before removing duplicates. `rowid` is only a deterministic
    // tie-breaker; freshness is defined by the persisted document timestamps.
    conn.execute_batch(
        "UPDATE documents AS d_keep
         SET title = COALESCE(
                 (
                     SELECT d_title.title
                     FROM documents AS d_title
                     WHERE d_title.url = d_keep.url
                       AND d_title.title IS NOT NULL
                       AND TRIM(d_title.title) != ''
                     ORDER BY julianday(d_title.updated_at) DESC,
                              d_title.updated_at DESC,
                              julianday(d_title.captured_at) DESC,
                              d_title.captured_at DESC,
                              d_title.rowid DESC
                     LIMIT 1
                 ),
                 d_keep.title
             ),
             raw_content = (
                 SELECT d_latest.raw_content
                 FROM documents AS d_latest
                 WHERE d_latest.url = d_keep.url
                 ORDER BY julianday(d_latest.updated_at) DESC,
                          d_latest.updated_at DESC,
                          julianday(d_latest.captured_at) DESC,
                          d_latest.captured_at DESC,
                          d_latest.rowid DESC
                 LIMIT 1
             ),
             source_version = (
                 SELECT d_latest.source_version
                 FROM documents AS d_latest
                 WHERE d_latest.url = d_keep.url
                 ORDER BY julianday(d_latest.updated_at) DESC,
                          d_latest.updated_at DESC,
                          julianday(d_latest.captured_at) DESC,
                          d_latest.captured_at DESC,
                          d_latest.rowid DESC
                 LIMIT 1
             ),
             captured_at = (
                 SELECT d_latest.captured_at
                 FROM documents AS d_latest
                 WHERE d_latest.url = d_keep.url
                 ORDER BY julianday(d_latest.updated_at) DESC,
                          d_latest.updated_at DESC,
                          julianday(d_latest.captured_at) DESC,
                          d_latest.captured_at DESC,
                          d_latest.rowid DESC
                 LIMIT 1
             ),
             updated_at = (
                 SELECT d_latest.updated_at
                 FROM documents AS d_latest
                 WHERE d_latest.url = d_keep.url
                 ORDER BY julianday(d_latest.updated_at) DESC,
                          d_latest.updated_at DESC,
                          julianday(d_latest.captured_at) DESC,
                          d_latest.captured_at DESC,
                          d_latest.rowid DESC
                 LIMIT 1
             )
         WHERE d_keep.rowid = (
             SELECT MIN(d_first.rowid)
             FROM documents AS d_first
             WHERE d_first.url = d_keep.url
         )
           AND EXISTS (
             SELECT 1
             FROM documents AS d_duplicate
             WHERE d_duplicate.url = d_keep.url
               AND d_duplicate.rowid != d_keep.rowid
         )",
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    conn.execute_batch(
        "UPDATE items
         SET document_id = (
             SELECT d_keep.id
             FROM documents d_dup
             JOIN documents d_keep ON d_dup.url = d_keep.url
             WHERE d_dup.id = items.document_id
               AND d_keep.rowid = (SELECT MIN(rowid) FROM documents WHERE url = d_dup.url)
         )
         WHERE document_id IN (
             SELECT id FROM documents
             WHERE rowid NOT IN (SELECT MIN(rowid) FROM documents GROUP BY url)
         )",
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute_batch(
        "DELETE FROM documents WHERE rowid NOT IN (SELECT MIN(rowid) FROM documents GROUP BY url)",
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute_batch("DROP INDEX IF EXISTS idx_documents_url")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute_batch("DROP INDEX IF EXISTS idx_documents_url_migration")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_url ON documents(url)")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn foreign_key_exists(
    conn: &Connection,
    table: &str,
    from_column: &str,
    referenced_table: &str,
) -> InfraResult<bool> {
    if !ALLOWED_FOREIGN_KEY_TABLES.contains(&table) {
        return Err(InfraError::Database(format!("unknown table: {table}")));
    }
    let mut stmt = conn
        .prepare(&format!("PRAGMA foreign_key_list({table})"))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let fks = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(2)?, row.get::<_, String>(3)?))
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    for fk in fks {
        let (target_table, source_column) = fk.map_err(|e| InfraError::Database(e.to_string()))?;
        if target_table == referenced_table && source_column == from_column {
            return Ok(true);
        }
    }

    Ok(false)
}

fn migrate_items_document_fk(conn: &Connection) -> InfraResult<()> {
    if foreign_key_exists(conn, "items", "document_id", "documents")? {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS items_ai;
        DROP TRIGGER IF EXISTS items_ad;
        DROP TRIGGER IF EXISTS items_au;
        UPDATE items
        SET document_id = NULL
        WHERE document_id IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM documents WHERE documents.id = items.document_id
          );
        ALTER TABLE items RENAME TO items_without_document_fk;
        CREATE TABLE items (
            id TEXT PRIMARY KEY,
            item_type TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            content TEXT NOT NULL,
            tags TEXT NOT NULL,
            source TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            document_id TEXT,
            excerpt TEXT,
            FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE SET NULL
        );
        INSERT INTO items
          (rowid, id, item_type, title, summary, content, tags, source,
           created_at, updated_at, document_id, excerpt)
        SELECT rowid, id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt
        FROM items_without_document_fk;
        DROP TABLE items_without_document_fk;
        CREATE INDEX IF NOT EXISTS idx_items_type ON items(item_type);
        CREATE INDEX IF NOT EXISTS idx_items_created ON items(created_at);
        CREATE INDEX IF NOT EXISTS idx_items_document ON items(document_id);
        CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
            INSERT INTO items_fts(rowid, title, summary, content, tags)
            VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
            VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
            VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
            INSERT INTO items_fts(rowid, title, summary, content, tags)
            VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
        END;
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}

fn migrate_extraction_jobs_conversation_fk(conn: &Connection) -> InfraResult<()> {
    if foreign_key_exists(conn, "extraction_jobs", "conversation_id", "conversations")? {
        return Ok(());
    }

    conn.execute_batch(
        r#"
        ALTER TABLE extraction_jobs RENAME TO extraction_jobs_without_conversation_fk;
        CREATE TABLE extraction_jobs (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL,
            mode TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            error TEXT,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            lease_owner TEXT,
            lease_expires_at TEXT,
            FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
        );
        INSERT INTO extraction_jobs
          (id, conversation_id, mode, status, created_at, updated_at, error,
           attempt_count, lease_owner, lease_expires_at)
        SELECT id, conversation_id, mode, status, created_at, updated_at, error,
               attempt_count, lease_owner, lease_expires_at
        FROM extraction_jobs_without_conversation_fk
        WHERE EXISTS (
            SELECT 1
            FROM conversations
            WHERE conversations.id = extraction_jobs_without_conversation_fk.conversation_id
        );
        DROP TABLE extraction_jobs_without_conversation_fk;
        CREATE INDEX IF NOT EXISTS idx_extraction_jobs_conversation
        ON extraction_jobs(conversation_id, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_extraction_jobs_recovery
        ON extraction_jobs(status, lease_expires_at, created_at);
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}

fn migrate_extraction_jobs_add_lease_columns(conn: &Connection) -> InfraResult<()> {
    if !column_exists(conn, "extraction_jobs", "attempt_count")? {
        conn.execute_batch(
            "ALTER TABLE extraction_jobs ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    if !column_exists(conn, "extraction_jobs", "lease_owner")? {
        conn.execute_batch("ALTER TABLE extraction_jobs ADD COLUMN lease_owner TEXT")
            .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    if !column_exists(conn, "extraction_jobs", "lease_expires_at")? {
        conn.execute_batch("ALTER TABLE extraction_jobs ADD COLUMN lease_expires_at TEXT")
            .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_extraction_jobs_recovery
        ON extraction_jobs(status, lease_expires_at, created_at);
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{column_exists, migrate_documents_url_unique, prepare_sqlite_db};
    use rusqlite::{Connection, OpenFlags};

    #[test]
    fn prepare_sqlite_db_rebuilds_legacy_items_with_document_fk() {
        let conn = Connection::open_in_memory()
            .unwrap_or_else(|err| panic!("open in-memory sqlite: {err}"));
        conn.execute_batch(
            r#"
            CREATE TABLE documents (
                id TEXT PRIMARY KEY,
                title TEXT,
                raw_content TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE items (
                id TEXT PRIMARY KEY,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                document_id TEXT,
                excerpt TEXT
            );
            INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
            VALUES
              ('orphan', 'knowledge', 'T', 'S', 'C', '[]', NULL,
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'missing-doc', NULL);
            "#,
        )
        .unwrap_or_else(|err| panic!("seed legacy schema: {err}"));

        prepare_sqlite_db(&conn).unwrap_or_else(|err| panic!("prepare sqlite db: {err}"));

        assert!(column_exists(&conn, "documents", "source_version")
            .expect("inspect migrated documents columns"));

        let document_id: Option<String> = conn
            .query_row(
                "SELECT document_id FROM items WHERE id = 'orphan'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("read migrated item: {err}"));
        assert_eq!(document_id, None);

        let fk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('items')
                 WHERE \"from\" = 'document_id' AND \"table\" = 'documents'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("read items fks: {err}"));
        assert_eq!(fk_count, 1);

        let err = conn
            .execute(
                "INSERT INTO items
                 (id, item_type, title, summary, content, tags, source,
                  created_at, updated_at, document_id, excerpt)
                 VALUES
                 ('new-orphan', 'knowledge', 'T', 'S', 'C', '[]', NULL,
                  '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'missing-doc', NULL)",
                [],
            )
            .expect_err("document FK should reject new orphan item");
        assert!(
            err.to_string().contains("FOREIGN KEY constraint failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn documents_url_migration_is_read_only_after_unique_index_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("refine.db");
        {
            let conn = Connection::open(&path).expect("open writable database");
            prepare_sqlite_db(&conn).expect("prepare database");
        }

        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open read-only database");
        migrate_documents_url_unique(&conn)
            .expect("an already-migrated database must not require writes");
    }

    #[test]
    fn documents_url_migration_merges_latest_snapshot_into_stable_identity() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        seed_duplicate_url_schema(&conn);
        conn.execute_batch(
            r#"
            INSERT INTO documents
              (id, title, raw_content, source, url, source_version,
               captured_at, created_at, updated_at)
            VALUES
              ('doc-old', 'Old title', 'old content', 'claude', 'https://example.com/thread', 'v1',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
              ('doc-titled', 'Current title', 'middle content', 'claude', 'https://example.com/thread', 'v2',
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z'),
              ('doc-latest', '   ', 'latest complete content', 'claude', 'https://example.com/thread', 'v3',
               '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z');
            INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
            VALUES
              ('item-old', 'knowledge', 'Old', 'S', 'C', '[]', NULL,
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'doc-old', NULL),
              ('item-middle', 'knowledge', 'Middle', 'S', 'C', '[]', NULL,
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 'doc-titled', NULL),
              ('item-latest', 'knowledge', 'Latest', 'S', 'C', '[]', NULL,
               '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z', 'doc-latest', NULL);
            "#,
        )
        .expect("seed duplicate documents");

        prepare_sqlite_db(&conn).expect("migrate duplicate documents");

        let document = conn
            .query_row(
                "SELECT id, title, raw_content, source_version, captured_at, created_at, updated_at
                 FROM documents WHERE url = 'https://example.com/thread'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .expect("read migrated document");
        assert_eq!(document.0, "doc-old", "stable document identity changed");
        assert_eq!(document.1.as_deref(), Some("Current title"));
        assert_eq!(document.2, "latest complete content");
        assert_eq!(document.3.as_deref(), Some("v3"));
        assert_eq!(document.4, "2026-01-03T00:00:00Z");
        assert_eq!(document.5, "2026-01-01T00:00:00Z");
        assert_eq!(document.6, "2026-01-03T00:00:00Z");

        let document_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .expect("count documents");
        assert_eq!(document_count, 1);
        let reattached_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE document_id = 'doc-old'",
                [],
                |row| row.get(0),
            )
            .expect("count reattached items");
        assert_eq!(reattached_count, 3);
        let unique_index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('documents')
                 WHERE name = 'idx_documents_url' AND \"unique\" = 1",
                [],
                |row| row.get(0),
            )
            .expect("inspect URL index");
        assert_eq!(unique_index_count, 1);
    }

    #[test]
    fn duplicate_url_migration_failure_rolls_back_merged_fields_and_item_links() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        seed_duplicate_url_schema(&conn);
        conn.execute_batch(
            r#"
            INSERT INTO documents
              (id, title, raw_content, source, url, source_version,
               captured_at, created_at, updated_at)
            VALUES
              ('doc-old', 'Old', 'old content', 'claude', 'https://example.com/thread', 'v1',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
              ('doc-new', 'New', 'new content', 'claude', 'https://example.com/thread', 'v2',
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');
            INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
            VALUES
              ('item-new', 'knowledge', 'T', 'S', 'C', '[]', NULL,
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 'doc-new', NULL);
            CREATE TRIGGER reject_duplicate_document_delete
            BEFORE DELETE ON documents
            WHEN OLD.id = 'doc-new'
            BEGIN
                SELECT RAISE(ABORT, 'injected migration failure');
            END;
            "#,
        )
        .expect("seed rollback fixture");

        let error = prepare_sqlite_db(&conn).expect_err("injected delete must fail migration");
        assert!(error.to_string().contains("injected migration failure"));

        let old_content: String = conn
            .query_row(
                "SELECT raw_content FROM documents WHERE id = 'doc-old'",
                [],
                |row| row.get(0),
            )
            .expect("read stable document after rollback");
        assert_eq!(old_content, "old content");
        let item_document_id: String = conn
            .query_row(
                "SELECT document_id FROM items WHERE id = 'item-new'",
                [],
                |row| row.get(0),
            )
            .expect("read item link after rollback");
        assert_eq!(item_document_id, "doc-new");
        let document_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .expect("count documents after rollback");
        assert_eq!(document_count, 2);
    }

    #[test]
    fn documents_url_migration_preserves_subsecond_timestamp_order_over_rowid_order() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        seed_duplicate_url_schema(&conn);
        conn.execute_batch(
            r#"
            INSERT INTO documents
              (id, title, raw_content, source, url, source_version,
               captured_at, created_at, updated_at)
            VALUES
              ('doc-newer', 'Newer', 'newer content', 'claude', 'https://example.com/subsecond', 'v2',
               '2026-01-01T00:00:00.900Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00.900Z'),
              ('doc-older', 'Older', 'older content', 'claude', 'https://example.com/subsecond', 'v1',
               '2026-01-01T00:00:00.100Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00.100Z');
            "#,
        )
        .expect("seed reverse-rowid subsecond fixture");

        prepare_sqlite_db(&conn).expect("migrate subsecond fixture");

        let snapshot = conn
            .query_row(
                "SELECT id, title, raw_content, source_version, captured_at, updated_at
                 FROM documents WHERE url = 'https://example.com/subsecond'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .expect("read subsecond migration result");
        assert_eq!(snapshot.0, "doc-newer");
        assert_eq!(snapshot.1.as_deref(), Some("Newer"));
        assert_eq!(snapshot.2, "newer content");
        assert_eq!(snapshot.3.as_deref(), Some("v2"));
        assert_eq!(snapshot.4, "2026-01-01T00:00:00.900Z");
        assert_eq!(snapshot.5, "2026-01-01T00:00:00.900Z");

        let migration_index_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('documents')
                 WHERE name = 'idx_documents_url_migration'",
                [],
                |row| row.get(0),
            )
            .expect("inspect temporary migration index");
        assert_eq!(migration_index_count, 0);
    }

    fn seed_duplicate_url_schema(conn: &Connection) {
        conn.execute_batch(include_str!("schema.sql"))
            .expect("seed duplicate URL schema");
    }

    #[test]
    fn prepare_sqlite_db_rebuilds_legacy_jobs_with_conversation_fk() {
        let conn = Connection::open_in_memory()
            .unwrap_or_else(|err| panic!("open in-memory sqlite: {err}"));
        conn.execute_batch(
            r#"
            CREATE TABLE conversations (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                title TEXT,
                raw_content TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                idempotency_key TEXT NOT NULL UNIQUE,
                item_ids TEXT NOT NULL,
                last_error TEXT
            );
            CREATE TABLE extraction_jobs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                error TEXT
            );
            INSERT INTO conversations
              (id, user_id, source, url, title, raw_content, metadata_json,
               captured_at, created_at, status, idempotency_key, item_ids, last_error)
            VALUES
              ('conv-1', 'user-1', 'test', 'https://example.com/1', NULL, 'raw', '{}',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'queued', 'idem-1', '[]', NULL);
            INSERT INTO extraction_jobs
              (id, conversation_id, mode, status, created_at, updated_at, error)
            VALUES
              ('valid-job', 'conv-1', 'auto', 'pending',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL),
              ('orphan-job', 'missing-conv', 'auto', 'pending',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL);
            "#,
        )
        .unwrap_or_else(|err| panic!("seed legacy job schema: {err}"));

        prepare_sqlite_db(&conn).unwrap_or_else(|err| panic!("prepare sqlite db: {err}"));

        let job_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM extraction_jobs", [], |row| row.get(0))
            .unwrap_or_else(|err| panic!("count migrated jobs: {err}"));
        assert_eq!(job_count, 1);

        let valid_job_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM extraction_jobs WHERE id = 'valid-job'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("count valid job: {err}"));
        assert_eq!(valid_job_count, 1);

        let fk_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('extraction_jobs')
                 WHERE \"from\" = 'conversation_id' AND \"table\" = 'conversations'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("read job fks: {err}"));
        assert_eq!(fk_count, 1);

        let err = conn
            .execute(
                "INSERT INTO extraction_jobs
                 (id, conversation_id, mode, status, created_at, updated_at, error)
                 VALUES
                 ('new-orphan-job', 'missing-conv', 'auto', 'pending',
                  '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', NULL)",
                [],
            )
            .expect_err("conversation FK should reject new orphan job");
        assert!(
            err.to_string().contains("FOREIGN KEY constraint failed"),
            "unexpected error: {err}"
        );
    }
}

// 公共 API
pub use contract::{
    contract_incompatible_message, is_contract_compatible, normalize_contract_major,
    normalize_conversation_input, trim_optional, trim_required_field, validate_contract_version,
    CreateConversationRequest, DocumentDetailDto, DocumentDto, ItemDto,
    NormalizedConversationInput, CONTRACT_VERSION, CONTRACT_VERSION_HEADER,
};
pub use db_migration::{migrate_stale_dbs, MigrationReport};
pub use llm::{
    build_llm_client_from_env, build_required_llm_client_from_env, ClaudeClient, LlmClient,
    OpenAIClient,
};
pub use llm_retry::{
    llm_with_retry, llm_with_retry_policy, LlmRetryPolicy, DEFAULT_MAX_RETRIES,
    DEFAULT_RETRY_BASE_DELAY_SECS,
};
pub(crate) use llm_retry::{llm_with_retry_policy_ref, LlmRetryBehavior};
pub use paths::{default_db_path, ensure_db_dir, resolve_db_path, stale_db_candidates};
pub use quota_state::{is_exhausted as is_quota_exhausted, set_exhausted as set_quota_exhausted};
pub use sqlite::SqliteStore;
