use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemId, ItemType, Tag};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::time::Duration;

pub(super) fn configure_connection(conn: &Connection, in_memory: bool) -> InfraResult<()> {
    conn.busy_timeout(Duration::from_secs(5))
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let pragma_sql = if in_memory {
        "PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA synchronous = NORMAL;"
    } else {
        "PRAGMA foreign_keys = ON;
         PRAGMA temp_store = MEMORY;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;"
    };

    conn.execute_batch(pragma_sql)
        .map_err(|e| InfraError::Database(e.to_string()))?;

    Ok(())
}

pub(super) fn to_fts_query(query: &str) -> String {
    if query.contains('*') || query.contains('"') {
        return query.to_string();
    }

    query
        .split_whitespace()
        .map(|word| format!("{}*", word))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn row_to_item(row: &rusqlite::Row) -> InfraResult<Item> {
    let id: String = row.get(0).map_err(|e| InfraError::Database(e.to_string()))?;
    let type_str: String = row.get(1).map_err(|e| InfraError::Database(e.to_string()))?;
    let title: String = row.get(2).map_err(|e| InfraError::Database(e.to_string()))?;
    let summary: String = row.get(3).map_err(|e| InfraError::Database(e.to_string()))?;
    let content: String = row.get(4).map_err(|e| InfraError::Database(e.to_string()))?;
    let tags_json: String = row.get(5).map_err(|e| InfraError::Database(e.to_string()))?;
    let source_json: Option<String> = row.get(6).map_err(|e| InfraError::Database(e.to_string()))?;
    let created_at_raw: String = row.get(7).map_err(|e| InfraError::Database(e.to_string()))?;
    let updated_at_raw: String = row.get(8).map_err(|e| InfraError::Database(e.to_string()))?;

    let item_type = match type_str.as_str() {
        "skill" => ItemType::Skill,
        "snippet" => ItemType::Snippet,
        _ => ItemType::Knowledge,
    };

    let tags: Vec<String> =
        serde_json::from_str(&tags_json).map_err(|e| InfraError::Serialization(e.to_string()))?;
    let tags = tags.iter().filter_map(|tag| Tag::try_new(tag)).collect();

    let source = source_json
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|e| InfraError::Serialization(e.to_string()))?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| InfraError::Serialization(format!("created_at 解析失败: {}", e)))?;
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| InfraError::Serialization(format!("updated_at 解析失败: {}", e)))?;

    Item::restore(
        ItemId::from_str(&id),
        item_type,
        title,
        summary,
        content,
        tags,
        source,
        created_at,
        updated_at,
    )
    .map_err(|e| InfraError::Serialization(e.to_string()))
}
