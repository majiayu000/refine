use super::rows::{row_to_item, to_fts_query};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemType};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

#[cfg(test)]
const FTS_BOOTSTRAP_USER_VERSION: i64 = super::super::FTS_BOOTSTRAP_USER_VERSION;

pub(super) fn init_schema(conn: &Connection) -> InfraResult<()> {
    super::super::prepare_sqlite_db(conn)
}

#[cfg(test)]
fn column_exists(conn: &Connection, table: &str, column: &str) -> InfraResult<bool> {
    super::super::column_exists(conn, table, column)
}

#[cfg(test)]
fn maybe_rebuild_fts_index(conn: &Connection) -> InfraResult<bool> {
    super::super::maybe_rebuild_fts_index(conn)
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
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let rows = tx
        .execute("DELETE FROM items WHERE id = ?1", [id])
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if rows > 0 {
        prune_item_id_from_conversations(&tx, id)?;
    }
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(rows > 0)
}

fn prune_item_id_from_conversations(conn: &Connection, item_id: &str) -> InfraResult<()> {
    let updates = {
        let mut stmt = conn
            .prepare("SELECT id, item_ids FROM conversations WHERE item_ids LIKE '%' || ?1 || '%'")
            .map_err(|e| InfraError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([item_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| InfraError::Database(e.to_string()))?;
        let mut updates = Vec::new();
        for row in rows {
            let (conversation_id, raw_item_ids) =
                row.map_err(|e| InfraError::Database(e.to_string()))?;
            let mut item_ids: Vec<String> = serde_json::from_str(&raw_item_ids)
                .map_err(|e| InfraError::Serialization(e.to_string()))?;
            let original_len = item_ids.len();
            item_ids.retain(|existing| existing != item_id);
            if item_ids.len() != original_len {
                let item_ids_json = serde_json::to_string(&item_ids)
                    .map_err(|e| InfraError::Serialization(e.to_string()))?;
                updates.push((conversation_id, item_ids_json));
            }
        }
        updates
    };

    for (conversation_id, item_ids_json) in updates {
        conn.execute(
            "UPDATE conversations SET item_ids = ?1 WHERE id = ?2",
            params![item_ids_json, conversation_id],
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    }

    Ok(())
}
pub(super) fn delete_by_document_id(conn: &Connection, document_id: &str) -> InfraResult<usize> {
    let item_ids = {
        let mut stmt = conn
            .prepare("SELECT id FROM items WHERE document_id = ?1")
            .map_err(|e| InfraError::Database(e.to_string()))?;
        let rows = stmt
            .query_map([document_id], |row| row.get::<_, String>(0))
            .map_err(|e| InfraError::Database(e.to_string()))?;
        rows.map(|row| row.map_err(|e| InfraError::Database(e.to_string())))
            .collect::<InfraResult<Vec<_>>>()?
    };

    let deleted = conn
        .execute("DELETE FROM items WHERE document_id = ?1", [document_id])
        .map_err(|e| InfraError::Database(e.to_string()))?;
    for item_id in item_ids {
        prune_item_id_from_conversations(conn, &item_id)?;
    }
    Ok(deleted)
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
pub(super) fn find_by_date_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare("SELECT id, item_type, title, summary, content, tags, source, created_at, updated_at, document_id, excerpt FROM items WHERE created_at BETWEEN ?1 AND ?2 ORDER BY created_at DESC")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([start.to_rfc3339(), end.to_rfc3339()], |row| {
            row_to_item(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}

pub(super) fn find_observations_by_event_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> InfraResult<Vec<Item>> {
    let mut stmt = conn
        .prepare(
            "SELECT i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source, i.created_at, i.updated_at, i.document_id, i.excerpt
             FROM items i
             LEFT JOIN documents d ON i.document_id = d.id
             WHERE i.item_type = ?1
               AND COALESCE(d.captured_at, i.created_at) >= ?2
               AND COALESCE(d.captured_at, i.created_at) < ?3
             ORDER BY COALESCE(d.captured_at, i.created_at) DESC, i.created_at DESC",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(
            params![
                ItemType::Observation.as_str(),
                start.to_rfc3339(),
                end.to_rfc3339()
            ],
            |row| row_to_item(row).map_err(to_row_err),
        )
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

    #[test]
    fn find_by_date_range_returns_items_in_range() {
        use super::{find_by_date_range, save};
        use crate::knowledge::{Item, ItemId, ItemType, RestoreParams};
        use chrono::{Duration, TimeZone, Utc};

        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        let base = Utc.with_ymd_and_hms(2026, 1, 10, 0, 0, 0).unwrap();

        let make_item = |days_offset: i64| {
            let ts = base + Duration::days(days_offset);
            Item::restore(RestoreParams {
                id: ItemId::new(),
                item_type: ItemType::Observation,
                title: format!("item +{}d", days_offset),
                summary: String::new(),
                content: String::new(),
                tags: vec![],
                source: None,
                document_id: None,
                excerpt: None,
                created_at: ts,
                updated_at: ts,
            })
            .unwrap()
        };

        save(&conn, &make_item(-5)).unwrap(); // 2026-01-05 — before range
        save(&conn, &make_item(0)).unwrap(); // 2026-01-10 — start boundary
        save(&conn, &make_item(5)).unwrap(); // 2026-01-15 — inside range
        save(&conn, &make_item(10)).unwrap(); // 2026-01-20 — end boundary
        save(&conn, &make_item(15)).unwrap(); // 2026-01-25 — after range

        let start = base;
        let end = base + Duration::days(10);
        let result = find_by_date_range(&conn, start, end).unwrap();

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn find_observations_by_event_range_uses_document_captured_at_before_item_created_at() {
        use super::{find_observations_by_event_range, save};
        use crate::knowledge::{DocumentId, Item, ItemId, ItemType, RestoreParams};
        use chrono::{Duration, TimeZone, Utc};

        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        let base = Utc.with_ymd_and_hms(2026, 5, 25, 0, 0, 0).unwrap();
        let historical_event = base - Duration::days(10);
        let current_ingest = base - Duration::days(1);
        let doc_id = DocumentId::from("doc-historical");

        conn.execute(
            "INSERT INTO documents (id, title, raw_content, source, url, captured_at, created_at, updated_at)
             VALUES (?1, 'Historical session', 'raw', 'codex-session', 'file:///session.jsonl', ?2, ?3, ?3)",
            (
                doc_id.as_str(),
                historical_event.to_rfc3339(),
                current_ingest.to_rfc3339(),
            ),
        )
        .expect("insert document");

        let linked_observation = Item::restore(RestoreParams {
            id: ItemId::from("linked-observation"),
            item_type: ItemType::Observation,
            title: "linked".to_string(),
            summary: String::new(),
            content: String::new(),
            tags: vec![],
            source: None,
            document_id: Some(doc_id),
            excerpt: None,
            created_at: current_ingest,
            updated_at: current_ingest,
        })
        .unwrap();
        save(&conn, &linked_observation).unwrap();

        let fallback_observation = Item::restore(RestoreParams {
            id: ItemId::from("fallback-observation"),
            item_type: ItemType::Observation,
            title: "fallback".to_string(),
            summary: String::new(),
            content: String::new(),
            tags: vec![],
            source: None,
            document_id: None,
            excerpt: None,
            created_at: current_ingest,
            updated_at: current_ingest,
        })
        .unwrap();
        save(&conn, &fallback_observation).unwrap();

        let this_week = find_observations_by_event_range(&conn, base - Duration::days(7), base)
            .expect("query current week");
        assert_eq!(this_week.len(), 1);
        assert_eq!(this_week[0].id().as_str(), "fallback-observation");

        let last_week = find_observations_by_event_range(
            &conn,
            base - Duration::days(14),
            base - Duration::days(7),
        )
        .expect("query previous week");
        assert_eq!(last_week.len(), 1);
        assert_eq!(last_week[0].id().as_str(), "linked-observation");
    }

    #[test]
    fn column_exists_rejects_unknown_table() {
        use super::column_exists;
        use crate::error::InfraError;

        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        let err = column_exists(&conn, "injected_table", "id").unwrap_err();
        match err {
            InfraError::Database(msg) => assert!(msg.contains("unknown table: injected_table")),
            other => panic!("expected Database error, got {other:?}"),
        }
    }

    #[test]
    fn find_by_date_range_empty_when_no_items_in_range() {
        use super::find_by_date_range;
        use chrono::{Duration, Utc};

        let conn = Connection::open_in_memory().expect("open in-memory db");
        init_schema(&conn).expect("init schema");

        let start = Utc::now() + Duration::days(100);
        let end = Utc::now() + Duration::days(200);
        let result = find_by_date_range(&conn, start, end).unwrap();

        assert!(result.is_empty());
    }
}
