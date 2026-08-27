use super::rows::row_to_item;
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{
    DocumentId, Item, ItemType, ObservationDocumentMeta, ObservationWindowSnapshot,
};
use chrono::{DateTime, Duration, Utc};
#[cfg(test)]
use rusqlite::params;
use rusqlite::{params_from_iter, Connection};
use std::collections::BTreeSet;

pub(super) fn load(
    conn: &Connection,
    cutoff: DateTime<Utc>,
    period_days: Option<usize>,
) -> InfraResult<ObservationWindowSnapshot> {
    let bounds = period_days.map(|days| {
        let days = i64::try_from(days)
            .map_err(|_| InfraError::Database("insights period exceeds i64".into()))?;
        let current_start = cutoff
            .checked_sub_signed(Duration::days(days))
            .ok_or_else(|| InfraError::Database("insights current window underflow".into()))?;
        let previous_start = current_start
            .checked_sub_signed(Duration::days(days))
            .ok_or_else(|| InfraError::Database("insights previous window underflow".into()))?;
        Ok::<_, InfraError>((current_start, previous_start))
    });
    let bounds = bounds.transpose()?;
    let transaction = conn
        .unchecked_transaction()
        .map_err(|error| InfraError::Database(error.to_string()))?;
    let (current, previous) = load_items(&transaction, cutoff, bounds)?;
    let documents = load_document_metadata(&transaction, &current, &previous)?;
    transaction
        .commit()
        .map_err(|error| InfraError::Database(error.to_string()))?;
    Ok(ObservationWindowSnapshot {
        current,
        previous,
        documents,
    })
}

fn load_items(
    conn: &Connection,
    cutoff: DateTime<Utc>,
    bounds: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> InfraResult<(Vec<Item>, Vec<Item>)> {
    let item_columns = "i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source, i.created_at, i.updated_at, i.document_id, i.excerpt";
    let (sql, values) = match bounds {
        Some((current_start, previous_start)) => (
            format!(
                "SELECT {item_columns}, CASE WHEN COALESCE(d.captured_at, i.created_at) >= ?2 THEN 0 ELSE 1 END
                 FROM items i LEFT JOIN documents d ON i.document_id = d.id
                 WHERE i.item_type = ?1
                   AND COALESCE(d.captured_at, i.created_at) >= ?3
                   AND COALESCE(d.captured_at, i.created_at) < ?4
                 ORDER BY COALESCE(d.captured_at, i.created_at), i.id"
            ),
            vec![
                ItemType::Observation.as_str().to_string(),
                current_start.to_rfc3339(),
                previous_start.to_rfc3339(),
                cutoff.to_rfc3339(),
            ],
        ),
        None => (
            format!(
                "SELECT {item_columns}, 0
                 FROM items i LEFT JOIN documents d ON i.document_id = d.id
                 WHERE i.item_type = ?1
                   AND COALESCE(d.captured_at, i.created_at) < ?2
                 ORDER BY COALESCE(d.captured_at, i.created_at), i.id"
            ),
            vec![
                ItemType::Observation.as_str().to_string(),
                cutoff.to_rfc3339(),
            ],
        ),
    };
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| InfraError::Database(error.to_string()))?;
    let rows = statement
        .query_map(params_from_iter(values.iter()), |row| {
            let item = row_to_item(row).map_err(to_row_error)?;
            let bucket: i64 = row.get(11)?;
            Ok((bucket, item))
        })
        .map_err(|error| InfraError::Database(error.to_string()))?;
    let mut current = Vec::new();
    let mut previous = Vec::new();
    for row in rows {
        let (bucket, item) = row.map_err(|error| InfraError::Database(error.to_string()))?;
        if bucket == 0 {
            current.push(item);
        } else {
            previous.push(item);
        }
    }
    Ok((current, previous))
}

fn load_document_metadata(
    conn: &Connection,
    current: &[Item],
    previous: &[Item],
) -> InfraResult<Vec<ObservationDocumentMeta>> {
    let document_ids: BTreeSet<String> = current
        .iter()
        .chain(previous)
        .filter_map(|item| item.document_id().map(|id| id.as_str().to_string()))
        .collect();
    if document_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", document_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, source, captured_at FROM documents WHERE id IN ({placeholders}) ORDER BY id"
    );
    let mut statement = conn
        .prepare(&sql)
        .map_err(|error| InfraError::Database(error.to_string()))?;
    let rows = statement
        .query_map(params_from_iter(document_ids.iter()), |row| {
            let id: String = row.get(0)?;
            let source: String = row.get(1)?;
            let captured_at: String = row.get(2)?;
            Ok((id, source, captured_at))
        })
        .map_err(|error| InfraError::Database(error.to_string()))?;
    rows.map(|row| {
        let (id, source, captured_at) =
            row.map_err(|error| InfraError::Database(error.to_string()))?;
        let captured_at = DateTime::parse_from_rfc3339(&captured_at)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| InfraError::Serialization(error.to_string()))?;
        Ok(ObservationDocumentMeta {
            id: DocumentId::from(id.as_str()),
            source,
            captured_at,
        })
    })
    .collect()
}

fn to_row_error(error: InfraError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{ItemId, RestoreParams};
    use chrono::TimeZone;

    #[test]
    fn snapshot_reads_equal_windows_metadata_and_bounds_all_history() {
        let conn = Connection::open_in_memory().unwrap();
        super::super::ops::init_schema(&conn).unwrap();
        let cutoff = Utc.with_ymd_and_hms(2026, 8, 27, 0, 0, 0).unwrap();
        for (id, source, days_ago) in [
            ("current", "codex-session", 1),
            ("previous", "claude-code-session", 8),
            ("future", "codex-session", -1),
        ] {
            let event_time = cutoff - Duration::days(days_ago);
            conn.execute(
                "INSERT INTO documents (id,title,raw_content,source,url,captured_at,created_at,updated_at) VALUES (?1,?1,'raw',?2,?1,?3,?3,?3)",
                params![id, source, event_time.to_rfc3339()],
            )
            .unwrap();
            let item = Item::restore(RestoreParams {
                id: ItemId::from(id),
                item_type: ItemType::Observation,
                title: id.into(),
                summary: String::new(),
                content: String::new(),
                tags: Vec::new(),
                source: None,
                document_id: Some(DocumentId::from(id)),
                excerpt: None,
                created_at: event_time,
                updated_at: event_time,
            })
            .unwrap();
            super::super::ops::save(&conn, &item).unwrap();
        }

        let delta = load(&conn, cutoff, Some(7)).unwrap();
        assert_eq!(delta.current[0].id().as_str(), "current");
        assert_eq!(delta.previous[0].id().as_str(), "previous");
        assert_eq!(delta.documents.len(), 2);

        let all = load(&conn, cutoff, None).unwrap();
        assert_eq!(all.current.len(), 2);
        assert!(all
            .current
            .iter()
            .all(|item| item.id().as_str() != "future"));
        assert!(all.previous.is_empty());
    }
}
