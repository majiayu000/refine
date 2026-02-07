use super::rows::{row_to_item, to_fts_query};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemType};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

pub(super) fn init_schema(conn: &Connection) -> InfraResult<()> {
    conn.execute_batch(include_str!("../schema.sql"))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    conn.execute("INSERT INTO items_fts(items_fts) VALUES('rebuild')", [])
        .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}
pub(super) fn find_by_id(conn: &Connection, id: &str) -> InfraResult<Option<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at FROM items WHERE id = ?1")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    stmt.query_row([id], |row| row_to_item(row).map_err(to_row_err))
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))
}
pub(super) fn find_all(conn: &Connection) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at FROM items ORDER BY created_at DESC")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| row_to_item(row).map_err(to_row_err))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}
pub(super) fn find_by_type(conn: &Connection, item_type: ItemType) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at FROM items WHERE item_type = ?1 ORDER BY created_at DESC")
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
                .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at FROM items WHERE item_type = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3")
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
                .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at FROM items ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
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
    let all_items = find_all(conn)?;
    if tags.is_empty() {
        return Ok(all_items);
    }

    let required: Vec<String> = tags.iter().map(|tag| tag.to_lowercase()).collect();

    Ok(all_items
        .into_iter()
        .filter(|item| {
            let item_tags: HashSet<String> = item
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_lowercase())
                .collect();
            required.iter().all(|tag| item_tags.contains(tag))
        })
        .collect())
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
        "INSERT OR REPLACE INTO items (id, item_type, title, summary, content, tags, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            "SELECT i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source, i.created_at, i.updated_at
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
fn to_row_err(err: InfraError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}
