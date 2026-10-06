//! Receive revisions protect asynchronous capture publication from stale work.
//! All calls run inside the caller's SQLite transaction; model work stays outside.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{InfraError, InfraResult};
use crate::knowledge::Document;

pub(super) fn prepare(conn: &Connection) -> InfraResult<()> {
    if !super::column_exists(conn, "extraction_jobs", "source_revision")? {
        conn.execute_batch("ALTER TABLE extraction_jobs ADD COLUMN source_revision INTEGER")
            .map_err(database_error)?;
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS capture_receive_clock (
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             revision INTEGER NOT NULL CHECK(typeof(revision) = 'integer' AND revision >= 0),
             legacy_import INTEGER NOT NULL DEFAULT 0 CHECK(legacy_import IN (0, 1))
         );
         INSERT OR IGNORE INTO capture_receive_clock(singleton, revision) VALUES(1, 0);
         CREATE TABLE IF NOT EXISTS conversation_source_revisions (
             conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
             revision INTEGER NOT NULL CHECK(revision >= 0)
         );
         CREATE TABLE IF NOT EXISTS document_capture_publications (
             url TEXT PRIMARY KEY,
             revision INTEGER NOT NULL CHECK(revision >= 0),
             conversation_id TEXT
         );
         -- Legacy admission order is unknown. Never manufacture it from time or rowid.
         INSERT OR IGNORE INTO conversation_source_revisions(conversation_id, revision)
             SELECT id, 0 FROM conversations;
         INSERT OR IGNORE INTO document_capture_publications(url, revision, conversation_id)
             SELECT url, 0, NULL FROM documents;
         CREATE TRIGGER IF NOT EXISTS conversations_receive_revision_ai
         AFTER INSERT ON conversations BEGIN
             UPDATE capture_receive_clock SET revision = revision + 1
                 WHERE singleton = 1 AND legacy_import = 0;
             INSERT INTO conversation_source_revisions(conversation_id, revision)
                 SELECT NEW.id, CASE WHEN legacy_import = 1 THEN 0 ELSE revision END
                 FROM capture_receive_clock WHERE singleton = 1;
         END;
         CREATE TRIGGER IF NOT EXISTS conversations_receive_revision_au
         AFTER UPDATE OF raw_content, source, url, title, captured_at ON conversations
         WHEN OLD.raw_content IS NOT NEW.raw_content OR OLD.source IS NOT NEW.source
           OR OLD.url IS NOT NEW.url OR OLD.title IS NOT NEW.title
           OR OLD.captured_at IS NOT NEW.captured_at
         BEGIN
             UPDATE capture_receive_clock SET revision = revision + 1
                 WHERE singleton = 1 AND legacy_import = 0;
             INSERT INTO conversation_source_revisions(conversation_id, revision)
                 SELECT NEW.id, CASE WHEN legacy_import = 1 THEN 0 ELSE revision END
                 FROM capture_receive_clock WHERE singleton = 1
                 ON CONFLICT(conversation_id) DO UPDATE SET revision = excluded.revision;
         END;",
    )
    .map_err(database_error)
}

pub(super) fn set_legacy_import(conn: &Connection, enabled: bool) -> InfraResult<()> {
    if conn.is_autocommit() {
        return Err(database_error(
            "legacy capture import mode requires the import transaction",
        ));
    }
    let changed = conn
        .execute(
            "UPDATE capture_receive_clock SET legacy_import = ?1 WHERE singleton = 1",
            [enabled],
        )
        .map_err(database_error)?;
    if changed != 1 {
        return Err(database_error("capture receive clock is missing"));
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct Publication {
    conversation_id: String,
    revision: i64,
}

struct ClaimedCapture {
    conversation_id: String,
    claimed: Option<i64>,
    current: i64,
    url: String,
    source: String,
    content: String,
    captured_at: String,
    title: Option<String>,
}

pub(crate) fn authorize(
    conn: &Connection,
    job_id: &str,
    document: &Document,
) -> InfraResult<Publication> {
    let capture = conn
        .query_row(
            "SELECT c.id, j.source_revision, r.revision, c.url, c.source, c.raw_content,
                c.captured_at, c.title
         FROM extraction_jobs j JOIN conversations c ON c.id = j.conversation_id
         JOIN conversation_source_revisions r ON r.conversation_id = c.id
         WHERE j.id = ?1",
            [job_id],
            |row| {
                Ok(ClaimedCapture {
                    conversation_id: row.get(0)?,
                    claimed: row.get(1)?,
                    current: row.get(2)?,
                    url: row.get(3)?,
                    source: row.get(4)?,
                    content: row.get(5)?,
                    captured_at: row.get(6)?,
                    title: row.get(7)?,
                })
            },
        )
        .map_err(database_error)?;
    // Legacy claims have no verified order. Preserve 0-to-0 behavior, while a
    // positive publication always fences out later completion of legacy work.
    let claimed = capture.claimed.unwrap_or(0);
    if capture.current != claimed {
        return Err(rejected(&format!(
            "capture_source_changed: claimed revision {claimed}, current revision {}; capture input changed while the task was running",
            capture.current
        )));
    }
    let captured_at = DateTime::parse_from_rfc3339(&capture.captured_at)
        .map_err(|error| database_error(format!("invalid capture timestamp: {error}")))?
        .with_timezone(&Utc);
    if document.url() != capture.url
        || document.source() != capture.source
        || document.raw_content() != capture.content
        || document.captured_at() != captured_at
        || capture
            .title
            .as_deref()
            .is_some_and(|title| document.title() != Some(title))
    {
        return Err(rejected(
            "capture_source_changed: result document does not match the claimed capture input",
        ));
    }
    let published: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT revision, conversation_id FROM document_capture_publications WHERE url = ?1",
            [&capture.url],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(database_error)?;
    if let Some((revision, owner)) = published {
        if revision > claimed
            || (revision == claimed
                && claimed > 0
                && owner.as_deref() != Some(&capture.conversation_id))
        {
            return Err(rejected(&format!(
                "capture_superseded: received revision {claimed} cannot replace already published revision {revision}"
            )));
        }
    }
    Ok(Publication {
        conversation_id: capture.conversation_id,
        revision: claimed,
    })
}

pub(crate) fn record(
    conn: &Connection,
    document: &Document,
    publication: &Publication,
) -> InfraResult<()> {
    let changed = conn
        .execute(
            "INSERT INTO document_capture_publications(url, revision, conversation_id)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(url) DO UPDATE SET
             revision = excluded.revision, conversation_id = excluded.conversation_id
         WHERE document_capture_publications.revision < excluded.revision
            OR (document_capture_publications.revision = excluded.revision
                AND (excluded.revision = 0
                     OR document_capture_publications.conversation_id = excluded.conversation_id))",
            params![
                document.url(),
                publication.revision,
                publication.conversation_id
            ],
        )
        .map_err(database_error)?;
    if changed != 1 {
        return Err(rejected(
            "capture_superseded: publication revision changed during the result transaction",
        ));
    }
    Ok(())
}

fn database_error(error: impl std::fmt::Display) -> InfraError {
    InfraError::Database(error.to_string())
}

fn rejected(reason: &str) -> InfraError {
    InfraError::CapturePublicationRejected(reason.to_string())
}

#[cfg(test)]
mod tests;
