//! Keep capture results bound to the source revision actually claimed by a worker.
//! The existing capture_revisions sequence is the only publication clock.

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
        "CREATE TABLE IF NOT EXISTS capture_publication_context (
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             legacy_import INTEGER NOT NULL DEFAULT 0 CHECK(legacy_import IN (0, 1))
         );
         INSERT OR IGNORE INTO capture_publication_context(singleton) VALUES(1);
         -- Historical acceptance order is unknown. An absent revision means 0.
         -- Existing positive revisions remain durable across future upgrades.
         INSERT OR IGNORE INTO document_capture_publications(url, revision, conversation_id)
             SELECT url, 0, '' FROM documents;
         DROP TRIGGER IF EXISTS conversations_capture_revision;
         DROP TRIGGER IF EXISTS conversations_capture_source_revision;
         CREATE TRIGGER conversations_capture_revision
         AFTER INSERT ON conversations
         WHEN (SELECT legacy_import FROM capture_publication_context WHERE singleton=1)=0
         BEGIN
             INSERT INTO capture_revisions(conversation_id) VALUES(NEW.id);
         END;
         CREATE TRIGGER conversations_capture_source_revision
         AFTER UPDATE OF raw_content, source, url, title, captured_at ON conversations
         WHEN OLD.raw_content IS NOT NEW.raw_content OR OLD.source IS NOT NEW.source
           OR OLD.url IS NOT NEW.url OR OLD.title IS NOT NEW.title
           OR OLD.captured_at IS NOT NEW.captured_at
         BEGIN
             DELETE FROM capture_revisions WHERE conversation_id=NEW.id;
             INSERT INTO capture_revisions(conversation_id)
                 SELECT NEW.id FROM capture_publication_context
                 WHERE singleton=1 AND legacy_import=0;
         END;",
    )
    .map_err(database_error)
}

pub(super) fn set_legacy_import(conn: &Connection, enabled: bool) -> InfraResult<()> {
    if conn.is_autocommit() {
        return Err(database_error(
            "legacy capture import requires the import transaction",
        ));
    }
    let changed = conn
        .execute(
            "UPDATE capture_publication_context SET legacy_import=?1 WHERE singleton=1",
            [enabled],
        )
        .map_err(database_error)?;
    if changed != 1 {
        return Err(database_error("capture publication context is missing"));
    }
    Ok(())
}

pub(crate) struct ClaimedSource {
    pub(crate) conversation_id: String,
    pub(crate) url: String,
    pub(crate) revision: i64,
}

struct CaptureInput {
    source: ClaimedSource,
    claimed: Option<i64>,
    provider: String,
    content: String,
    captured_at: String,
    title: Option<String>,
}

pub(crate) fn claimed_source(
    conn: &Connection,
    job_id: &str,
    owner: &str,
    document: &Document,
) -> InfraResult<Option<ClaimedSource>> {
    let capture = conn
        .query_row(
            "SELECT c.id, c.url, COALESCE(r.revision,0), j.source_revision,
                    c.source, c.raw_content, c.captured_at, c.title
             FROM extraction_jobs j JOIN conversations c ON c.id=j.conversation_id
             LEFT JOIN capture_revisions r ON r.conversation_id=c.id
             WHERE j.id=?1 AND j.status='running' AND j.lease_owner=?2",
            params![job_id, owner],
            |row| {
                Ok(CaptureInput {
                    source: ClaimedSource {
                        conversation_id: row.get(0)?,
                        url: row.get(1)?,
                        revision: row.get(2)?,
                    },
                    claimed: row.get(3)?,
                    provider: row.get(4)?,
                    content: row.get(5)?,
                    captured_at: row.get(6)?,
                    title: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(database_error)?;
    let Some(capture) = capture else {
        return Ok(None);
    };
    let claimed = capture.claimed.unwrap_or(0);
    if capture.source.revision != claimed {
        return Err(rejected(format!(
            "capture_source_changed: claimed revision {claimed}, current revision {}; capture input changed while the task was running",
            capture.source.revision
        )));
    }
    let captured_at = DateTime::parse_from_rfc3339(&capture.captured_at)
        .map_err(|error| database_error(format!("invalid capture timestamp: {error}")))?
        .with_timezone(&Utc);
    if document.url() != capture.source.url
        || document.source() != capture.provider
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
    Ok(Some(capture.source))
}

fn database_error(error: impl std::fmt::Display) -> InfraError {
    InfraError::Database(error.to_string())
}

fn rejected(reason: impl Into<String>) -> InfraError {
    InfraError::CapturePublicationRejected(reason.into())
}

#[cfg(test)]
mod tests;
