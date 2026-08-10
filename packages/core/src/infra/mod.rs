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
const ALLOWED_COLUMN_EXISTS_TABLES: &[&str] = &["items", "documents"];
const ALLOWED_FOREIGN_KEY_TABLES: &[&str] = &["items", "extraction_jobs"];

pub fn prepare_sqlite_db(conn: &Connection) -> InfraResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;

    tx.execute_batch(include_str!("schema.sql"))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    migrate_items_add_document_columns(&tx)?;
    migrate_documents_url_unique(&tx)?;
    migrate_items_document_fk(&tx)?;
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
            FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
        );
        INSERT INTO extraction_jobs
          (id, conversation_id, mode, status, created_at, updated_at, error)
        SELECT id, conversation_id, mode, status, created_at, updated_at, error
        FROM extraction_jobs_without_conversation_fk
        WHERE EXISTS (
            SELECT 1
            FROM conversations
            WHERE conversations.id = extraction_jobs_without_conversation_fk.conversation_id
        );
        DROP TABLE extraction_jobs_without_conversation_fk;
        CREATE INDEX IF NOT EXISTS idx_extraction_jobs_conversation
        ON extraction_jobs(conversation_id, created_at DESC);
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{migrate_documents_url_unique, prepare_sqlite_db};
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
pub use paths::{default_db_path, ensure_db_dir, resolve_db_path, stale_db_candidates};
pub use quota_state::{is_exhausted as is_quota_exhausted, set_exhausted as set_quota_exhausted};
pub use sqlite::SqliteStore;
