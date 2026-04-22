use super::rows::to_fts_query;
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Document, DocumentId, RestoreDocumentParams};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

pub(super) fn save(conn: &Connection, doc: &Document) -> InfraResult<()> {
    conn.execute(
        "INSERT INTO documents (id, title, raw_content, source, url, captured_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(url) DO UPDATE SET
             title = excluded.title,
             raw_content = excluded.raw_content,
             captured_at = excluded.captured_at,
             updated_at = excluded.updated_at",
        params![
            doc.id().as_str(),
            doc.title(),
            doc.raw_content(),
            doc.source(),
            doc.url(),
            doc.captured_at().to_rfc3339(),
            doc.created_at().to_rfc3339(),
            doc.updated_at().to_rfc3339(),
        ],
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

pub(super) fn find_by_id(conn: &Connection, id: &str) -> InfraResult<Option<Document>> {
    let mut stmt = conn
        .prepare("SELECT id, title, raw_content, source, url, captured_at, created_at, updated_at FROM documents WHERE id = ?1")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    stmt.query_row([id], |row| row_to_document(row).map_err(to_row_err))
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn find_by_url(conn: &Connection, url: &str) -> InfraResult<Option<Document>> {
    let mut stmt = conn
        .prepare("SELECT id, title, raw_content, source, url, captured_at, created_at, updated_at FROM documents WHERE url = ?1")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    stmt.query_row([url], |row| row_to_document(row).map_err(to_row_err))
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn find_recent(
    conn: &Connection,
    offset: usize,
    limit: usize,
) -> InfraResult<Vec<Document>> {
    let limit = std::cmp::min(limit, i64::MAX as usize) as i64;
    let offset = std::cmp::min(offset, i64::MAX as usize) as i64;

    let mut stmt = conn
        .prepare("SELECT id, title, raw_content, source, url, captured_at, created_at, updated_at FROM documents ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(params![limit, offset], |row| {
            row_to_document(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}

pub(super) fn count(conn: &Connection) -> InfraResult<usize> {
    let c: i64 = conn
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(c.max(0) as usize)
}

pub(super) fn delete(conn: &Connection, id: &str) -> InfraResult<bool> {
    let rows = conn
        .execute("DELETE FROM documents WHERE id = ?1", [id])
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(rows > 0)
}

pub(super) fn search_text(
    conn: &Connection,
    query: &str,
    offset: usize,
    limit: usize,
) -> InfraResult<Vec<Document>> {
    let limit = std::cmp::min(limit, i64::MAX as usize) as i64;
    let offset = std::cmp::min(offset, i64::MAX as usize) as i64;
    let Some(fts_query) = to_fts_query(query) else {
        return Ok(Vec::new());
    };

    let mut stmt = conn
        .prepare(
            "SELECT d.id, d.title, d.raw_content, d.source, d.url, d.captured_at, d.created_at, d.updated_at
             FROM documents d
             JOIN documents_fts fts ON d.rowid = fts.rowid
             WHERE documents_fts MATCH ?1
             ORDER BY fts.rank
             LIMIT ?2 OFFSET ?3",
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(params![fts_query, limit, offset], |row| {
            row_to_document(row).map_err(to_row_err)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string())))
        .collect()
}

pub(super) fn count_text_hits(conn: &Connection, query: &str) -> InfraResult<usize> {
    let Some(fts_query) = to_fts_query(query) else {
        return Ok(0);
    };
    let c: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM documents_fts WHERE documents_fts MATCH ?1",
            [fts_query],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(c.max(0) as usize)
}

fn row_to_document(row: &rusqlite::Row) -> InfraResult<Document> {
    let id: String = row
        .get(0)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let title: Option<String> = row
        .get(1)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let raw_content: String = row
        .get(2)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let source: String = row
        .get(3)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let url: String = row
        .get(4)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let captured_at_raw: String = row
        .get(5)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let created_at_raw: String = row
        .get(6)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let updated_at_raw: String = row
        .get(7)
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let parse_dt = |raw: &str, label: &str| -> InfraResult<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| InfraError::Serialization(format!("{} 解析失败: {}", label, e)))
    };

    Ok(Document::restore(RestoreDocumentParams {
        id: DocumentId::from(id.as_str()),
        title,
        raw_content,
        source,
        url,
        captured_at: parse_dt(&captured_at_raw, "captured_at")?,
        created_at: parse_dt(&created_at_raw, "created_at")?,
        updated_at: parse_dt(&updated_at_raw, "updated_at")?,
    }))
}

fn to_row_err(err: InfraError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}
