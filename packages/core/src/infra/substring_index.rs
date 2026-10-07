use crate::error::{InfraError, InfraResult};
use rusqlite::{Connection, OptionalExtension};

/// Runs inside the startup migration transaction, after any items table rebuild.
pub(super) fn prepare(conn: &Connection) -> InfraResult<()> {
    let existing: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('items_substring_fts', 'documents_substring_fts')",
        [], |row| row.get(0),
    ).map_err(database_error)?;
    conn.execute_batch(include_str!("substring_index.sql"))
        .map_err(database_error)?;
    let version: Option<i64> = conn
        .query_row(
            "SELECT version FROM search_index_versions WHERE name = 'substring'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(database_error)?;
    if existing != 2 || version != Some(1) {
        // The former startup migration only rebuilt items_fts. Backfill legacy documents too.
        for table in [
            "items_substring_fts",
            "documents_substring_fts",
            "documents_fts",
        ] {
            conn.execute(
                &format!("INSERT INTO {table}({table}) VALUES ('rebuild')"),
                [],
            )
            .map_err(database_error)?;
        }
        conn.execute(
            "INSERT INTO search_index_versions(name, version) VALUES ('substring', 1) ON CONFLICT(name) DO UPDATE SET version = excluded.version", [],
        ).map_err(database_error)?;
    }
    Ok(())
}

fn database_error(error: rusqlite::Error) -> InfraError {
    InfraError::Database(error.to_string())
}
