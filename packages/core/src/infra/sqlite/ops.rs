use super::rows::{row_to_item, to_fts_query};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemType};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

const FTS_BOOTSTRAP_USER_VERSION: i64 = 1;

pub(super) fn init_schema(conn: &Connection) -> InfraResult<()> {
    conn.execute_batch(include_str!("../schema.sql"))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    migrate_items_add_document_columns(conn)?;
    migrate_documents_url_unique(conn)?;
    let _ = maybe_rebuild_fts_index(conn)?;

    Ok(())
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
    // Remap items that point to a duplicate document to the kept document (earliest rowid
    // per URL) before deleting duplicates, so no items become orphans after the migration.
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
    // Remove duplicate URLs keeping the earliest-inserted row before adding constraint.
    conn.execute_batch(
        "DELETE FROM documents WHERE rowid NOT IN (SELECT MIN(rowid) FROM documents GROUP BY url)",
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    // Drop old non-unique index (may not exist on fresh databases).
    conn.execute_batch("DROP INDEX IF EXISTS idx_documents_url")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    // Create unique index so concurrent inserts of the same URL fail fast.
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_url ON documents(url)",
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> InfraResult<bool> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| InfraError::Database(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(names.iter().any(|n| n == column))
}

fn maybe_rebuild_fts_index(conn: &Connection) -> InfraResult<bool> {
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
pub(super) fn find_by_id(conn: &Connection, id: &str) -> InfraResult<Option<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE id = ?1")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    stmt.query_row([id], |row| row_to_item(row).map_err(to_row_err))
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))
}
pub(super) fn find_all(conn: &Connection) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items ORDER BY created_at DESC")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| row_to_item(row).map_err(to_row_err))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn find_by_type(conn: &Connection, item_type: ItemType) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE item_type = ?1 ORDER BY created_at DESC")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([item_type.as_str()], |row| {
            row_to_item(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn find_recent(
    conn: &Connection,
    item_type: Option<ItemType>,
    offset: usize,
    limit: usize,
) -> InfraResult<Vec<Item>> {
    let limit = std::cmp::min(limit, i64::MAX as usize) as i64;
    let offset = std::cmp::min(offset, i64::MAX as usize) as i64;

    match item_type {
        Some(item_type) => {
            let mut stmt = conn
                .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE item_type = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3")
                .map_err(|e| InfraError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![item_type.as_str(), limit, offset], |row| {
                    row_to_item(row).map_err(to_row_err)
                })
                .map_err(|e| InfraError::Database(e.to_string()))?;
            rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
                .collect()
        }
        None => {
            let mut stmt = conn
                .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
                .map_err(|e| InfraError::Database(e.to_string()))?;
            let rows = stmt
                .query_map(params![limit, offset], |row| {
                    row_to_item(row).map_err(to_row_err)
                })
                .map_err(|e| InfraError::Database(e.to_string()))?;
            rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
                .collect()
        }
    }
}
pub(super) fn count_items(conn: &Connection, item_type: Option<ItemType>) -> InfraResult<usize> {
    let count: i64 = match item_type {
        Some(item_type) => conn
            .query_row(
                "SELECT COUNT(*) FROM items WHERE item_type = ?1",
                [item_type.as_str()],
                |row| row.get(0),
            )
            .map_err(|e| InfraError::Database(e.to_string()))?,
        None => conn
            .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .map_err(|e| InfraError::Database(e.to_string()))?,
    };

    Ok(count.max(0) as usize)
}
pub(super) fn find_by_tags(conn: &Connection, tags: &[String]) -> InfraResult<Vec<Item>> {
    if tags.is_empty() {
        return find_all(conn);
    }

    let required: Vec<String> = tags
        .iter()
        .map(|tag| tag.trim().to_lowercase())
        .filter(|tag| !tag.is_empty())
        .collect();
    if required.is_empty() {
        return find_all(conn);
    }
    let required_json =
        serde_json::to_string(&required).map_err(|e| InfraError::Serialization(e.to_string()))?;

    let mut stmt = conn
        .prepare(
            "SELECT i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source, i.created_at, i.updated_at, i.document_id, i.excerpt
             FROM items i
             WHERE json_valid(i.tags)
               AND NOT EXISTS (
                   SELECT 1
                   FROM json_each(?1) AS required
                   WHERE LOWER(CAST(required.value AS TEXT)) NOT IN (
                       SELECT LOWER(CAST(item_tag.value AS TEXT))
                       FROM json_each(i.tags) AS item_tag
                   )
               )
             ORDER BY i.created_at DESC",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([required_json], |row| row_to_item(row).map_err(to_row_err))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn save(conn: &Connection, item: &Item) -> InfraResult<()> {
    let tags_json =
        serde_json::to_string(item.tags()).map_err(|e| InfraError::Serialization(e.to_string()))?;
    let source_json = item
        .source()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| InfraError::Serialization(e.to_string()))?;

    conn.execute(
        "INSERT OR REPLACE INTO items (id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            item.id().as_str(),
            item.item_type().as_str(),
            item.title(),
            item.summary(),
            item.content(),
            tags_json,
            source_json,
            item.created_at().to_rfc3339(),
            item.updated_at().to_rfc3339(),
            item.document_id().map(|id| id.as_str().to_string()),
            item.excerpt(),
        ],
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}
pub(super) fn delete(conn: &Connection, id: &str) -> InfraResult<bool> {
    let rows = conn
        .execute("DELETE FROM items WHERE id = ?1", [id])
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(rows > 0)
}
pub(super) fn exists(conn: &Connection, id: &str) -> InfraResult<bool> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM items WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(count > 0)
}
pub(super) fn search_text(
    conn: &Connection,
    query: &str,
    offset: usize,
    limit: usize,
) -> InfraResult<Vec<Item>> {
    let limit = std::cmp::min(limit, i64::MAX as usize) as i64;
    let offset = std::cmp::min(offset, i64::MAX as usize) as i64;
    let Some(fts_query) = to_fts_query(query) else {
        return Ok(Vec::new());
    };

    let mut stmt = conn
        .prepare(
            "SELECT i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source, i.created_at, i.updated_at, i.document_id, i.excerpt
             FROM items i
             JOIN items_fts fts ON i.rowid = fts.rowid
             WHERE items_fts MATCH ?1
             ORDER BY fts.rank
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(params![fts_query, limit, offset], |row| {
            row_to_item(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn count_text_hits(conn: &Connection, query: &str) -> InfraResult<usize> {
    let Some(fts_query) = to_fts_query(query) else {
        return Ok(0);
    };
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM items_fts WHERE items_fts MATCH ?1",
            [fts_query],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(count.max(0) as usize)
}
pub(super) fn find_since(conn: &Connection, since: DateTime<Utc>) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE created_at >= ?1 ORDER BY created_at DESC")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([since.to_rfc3339()], |row| {
            row_to_item(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn find_by_document_id(conn: &Connection, document_id: &str) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE document_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([document_id], |row| row_to_item(row).map_err(to_row_err))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}

fn to_row_err(err: InfraError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}

#[cfg(test)]
mod tests {
    use super::{init_schema, maybe_rebuild_fts_index, FTS_BOOTSTRAP_USER_VERSION};
    use rusqlite::Connection;

    const INSERT_ITEM_SQL: &str = r#"
        INSERT INTO items (id, item_type, title, summary, content, tags, source, created_at, updated_at)
        VALUES (?1, 'knowledge', 'title', 'summary', 'content', '["rust"]', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')
    "#;

    #[test]
    fn init_schema_skips_rebuild_when_fts_count_matches_items() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");
        conn.execute(INSERT_ITEM_SQL, ["item-1"])
            .expect("insert item");

        let rebuilt = maybe_rebuild_fts_index(&conn).expect("check fts state");
        assert!(!rebuilt);
    }

    #[test]
    fn init_schema_rebuilds_once_for_existing_rows_without_bootstrap_marker() {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");
        conn.execute(INSERT_ITEM_SQL, ["item-1"])
            .expect("insert item");
        conn.execute_batch("PRAGMA user_version = 0;")
            .expect("reset user_version");

        let rebuilt = maybe_rebuild_fts_index(&conn).expect("check fts state");
        assert!(rebuilt);

        let user_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read user_version");
        assert_eq!(user_version, FTS_BOOTSTRAP_USER_VERSION);

        let rebuilt_again = maybe_rebuild_fts_index(&conn).expect("check fts state again");
        assert!(!rebuilt_again);
    }
}
