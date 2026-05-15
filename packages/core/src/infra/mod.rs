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

pub fn prepare_sqlite_db(conn: &Connection) -> InfraResult<()> {
    conn.execute_batch(include_str!("schema.sql"))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    migrate_items_add_document_columns(conn)?;
    migrate_documents_url_unique(conn)?;
    let _ = maybe_rebuild_fts_index(conn)?;

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
