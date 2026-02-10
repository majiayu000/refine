use std::path::PathBuf;

use rusqlite::{params, Connection};

use crate::models::{
    ConversationRecord, ConversationStatus, EventRecord, ExtractionJobRecord, ExtractionMode,
    JobStatus,
};

pub struct ServerPersistence {
    db_path: PathBuf,
}

impl ServerPersistence {
    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        let persistence = Self { db_path };
        persistence.ensure_schema()?;
        Ok(persistence)
    }

    pub fn load_conversations(&self) -> Result<Vec<ConversationRecord>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, user_id, source, url, title, raw_content, metadata_json,
                       captured_at, created_at, status, idempotency_key, item_ids, last_error
                FROM conversations
                ORDER BY created_at DESC
                "#,
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let metadata_raw: String = row.get(6)?;
                let item_ids_raw: String = row.get(11)?;

                Ok(ConversationRecord {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    source: row.get(2)?,
                    url: row.get(3)?,
                    title: row.get(4)?,
                    raw_content: row.get(5)?,
                    metadata: serde_json::from_str(&metadata_raw)
                        .unwrap_or_else(|_| serde_json::json!({})),
                    captured_at: row.get(7)?,
                    created_at: row.get(8)?,
                    status: conversation_status_from_db(row.get::<_, String>(9)?.as_str()),
                    idempotency_key: row.get(10)?,
                    item_ids: serde_json::from_str(&item_ids_raw).unwrap_or_default(),
                    last_error: row.get(12)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut conversations = Vec::new();
        for row in rows {
            conversations.push(row.map_err(|e| e.to_string())?);
        }

        Ok(conversations)
    }

    pub fn load_jobs(&self) -> Result<Vec<ExtractionJobRecord>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, conversation_id, mode, status, created_at, updated_at, error
                FROM extraction_jobs
                ORDER BY created_at DESC
                "#,
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ExtractionJobRecord {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    mode: extraction_mode_from_db(row.get::<_, String>(2)?.as_str()),
                    status: job_status_from_db(row.get::<_, String>(3)?.as_str()),
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    error: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|e| e.to_string())?);
        }

        Ok(jobs)
    }

    pub fn upsert_conversation(&self, record: &ConversationRecord) -> Result<(), String> {
        let conn = self.open()?;
        let item_ids = serde_json::to_string(&record.item_ids).map_err(|e| e.to_string())?;
        let metadata = serde_json::to_string(&record.metadata).map_err(|e| e.to_string())?;

        conn.execute(
            r#"
            INSERT INTO conversations
              (id, user_id, source, url, title, raw_content, metadata_json,
               captured_at, created_at, status, idempotency_key, item_ids, last_error)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7,
               ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
              user_id=excluded.user_id,
              source=excluded.source,
              url=excluded.url,
              title=excluded.title,
              raw_content=excluded.raw_content,
              metadata_json=excluded.metadata_json,
              captured_at=excluded.captured_at,
              created_at=excluded.created_at,
              status=excluded.status,
              idempotency_key=excluded.idempotency_key,
              item_ids=excluded.item_ids,
              last_error=excluded.last_error
            "#,
            params![
                record.id,
                record.user_id,
                record.source,
                record.url,
                record.title,
                record.raw_content,
                metadata,
                record.captured_at,
                record.created_at,
                conversation_status_to_db(&record.status),
                record.idempotency_key,
                item_ids,
                record.last_error,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn upsert_job(&self, job: &ExtractionJobRecord) -> Result<(), String> {
        let conn = self.open()?;

        conn.execute(
            r#"
            INSERT INTO extraction_jobs
              (id, conversation_id, mode, status, created_at, updated_at, error)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
              conversation_id=excluded.conversation_id,
              mode=excluded.mode,
              status=excluded.status,
              created_at=excluded.created_at,
              updated_at=excluded.updated_at,
              error=excluded.error
            "#,
            params![
                job.id,
                job.conversation_id,
                extraction_mode_to_db(&job.mode),
                job_status_to_db(&job.status),
                job.created_at,
                job.updated_at,
                job.error,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn insert_event(&self, event: &EventRecord) -> Result<(), String> {
        let conn = self.open()?;
        let properties = serde_json::to_string(&event.properties).map_err(|e| e.to_string())?;

        conn.execute(
            r#"
            INSERT INTO events
              (id, user_id, event_name, source, properties_json, created_at)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                event.id,
                event.user_id,
                event.event_name,
                event.source,
                properties,
                event.created_at,
            ],
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn event_counts_since(&self, since: Option<&str>) -> Result<Vec<(String, usize)>, String> {
        let conn = self.open()?;

        if let Some(since) = since {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT event_name, COUNT(*)
                    FROM events
                    WHERE created_at >= ?1
                    GROUP BY event_name
                    ORDER BY event_name ASC
                    "#,
                )
                .map_err(|e| e.to_string())?;

            let rows = stmt
                .query_map([since], |row| {
                    let event_name: String = row.get(0)?;
                    let count: i64 = row.get(1)?;
                    Ok((event_name, count))
                })
                .map_err(|e| e.to_string())?;

            return collect_event_counts(rows);
        }

        let mut stmt = conn
            .prepare(
                r#"
                SELECT event_name, COUNT(*)
                FROM events
                GROUP BY event_name
                ORDER BY event_name ASC
                "#,
            )
            .map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([], |row| {
                let event_name: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((event_name, count))
            })
            .map_err(|e| e.to_string())?;

        collect_event_counts(rows)
    }

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    fn ensure_schema(&self) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                title TEXT,
                raw_content TEXT NOT NULL,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                idempotency_key TEXT NOT NULL UNIQUE,
                item_ids TEXT NOT NULL,
                last_error TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_conversations_status_created
            ON conversations(status, created_at DESC);

            CREATE INDEX IF NOT EXISTS idx_conversations_captured_at
            ON conversations(captured_at DESC);

            CREATE TABLE IF NOT EXISTS extraction_jobs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                error TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_extraction_jobs_conversation
            ON extraction_jobs(conversation_id, created_at DESC);

            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                event_name TEXT NOT NULL,
                source TEXT NOT NULL,
                properties_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_events_created_at
            ON events(created_at DESC);

            CREATE INDEX IF NOT EXISTS idx_events_event_name_created_at
            ON events(event_name, created_at DESC);
            "#,
        )
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}

fn conversation_status_to_db(status: &ConversationStatus) -> &'static str {
    match status {
        ConversationStatus::Captured => "captured",
        ConversationStatus::Queued => "queued",
        ConversationStatus::Processing => "processing",
        ConversationStatus::Processed => "processed",
        ConversationStatus::Failed => "failed",
    }
}

fn conversation_status_from_db(raw: &str) -> ConversationStatus {
    match raw {
        "captured" => ConversationStatus::Captured,
        "queued" => ConversationStatus::Queued,
        "processing" => ConversationStatus::Processing,
        "processed" => ConversationStatus::Processed,
        "failed" => ConversationStatus::Failed,
        _ => ConversationStatus::Failed,
    }
}

fn extraction_mode_to_db(mode: &ExtractionMode) -> &'static str {
    match mode {
        ExtractionMode::Auto => "auto",
        ExtractionMode::Knowledge => "knowledge",
        ExtractionMode::Skill => "skill",
        ExtractionMode::Snippet => "snippet",
    }
}

fn extraction_mode_from_db(raw: &str) -> ExtractionMode {
    match raw {
        "knowledge" => ExtractionMode::Knowledge,
        "skill" => ExtractionMode::Skill,
        "snippet" => ExtractionMode::Snippet,
        _ => ExtractionMode::Auto,
    }
}

fn job_status_to_db(status: &JobStatus) -> &'static str {
    match status {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Succeeded => "succeeded",
        JobStatus::Failed => "failed",
    }
}

fn job_status_from_db(raw: &str) -> JobStatus {
    match raw {
        "running" => JobStatus::Running,
        "succeeded" => JobStatus::Succeeded,
        "failed" => JobStatus::Failed,
        _ => JobStatus::Pending,
    }
}

fn collect_event_counts(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<(String, i64)>>,
) -> Result<Vec<(String, usize)>, String> {
    let mut counts = Vec::new();
    for row in rows {
        let (event_name, count) = row.map_err(|e| e.to_string())?;
        counts.push((event_name, count.max(0) as usize));
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::ServerPersistence;
    use crate::models::{now_iso, EventRecord};
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!("refine-server-persistence-test-{}.db", Uuid::new_v4()))
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
    }

    fn build_event(name: &str, created_at: &str) -> EventRecord {
        EventRecord {
            id: Uuid::new_v4().to_string(),
            user_id: "test-user".to_string(),
            event_name: name.to_string(),
            source: "extension".to_string(),
            properties: json!({"from": "test"}),
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn event_counts_since_returns_aggregated_counts() {
        let path = temp_db_path();
        let persistence = ServerPersistence::new(path.clone()).expect("persistence init failed");

        let now = now_iso();
        persistence
            .insert_event(&build_event("conversation_extracted", &now))
            .expect("insert event 1");
        persistence
            .insert_event(&build_event("conversation_extracted", &now))
            .expect("insert event 2");
        persistence
            .insert_event(&build_event("conversation_synced", &now))
            .expect("insert event 3");

        let counts = persistence
            .event_counts_since(None)
            .expect("query counts failed");

        let extracted = counts
            .iter()
            .find(|(name, _)| name == "conversation_extracted")
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let synced = counts
            .iter()
            .find(|(name, _)| name == "conversation_synced")
            .map(|(_, count)| *count)
            .unwrap_or(0);

        assert_eq!(extracted, 2);
        assert_eq!(synced, 1);

        cleanup(&path);
    }

    #[test]
    fn event_counts_since_filters_old_events() {
        let path = temp_db_path();
        let persistence = ServerPersistence::new(path.clone()).expect("persistence init failed");

        persistence
            .insert_event(&build_event("conversation_extracted", "2020-01-01T00:00:00Z"))
            .expect("insert old event");
        let recent = now_iso();
        persistence
            .insert_event(&build_event("conversation_extracted", &recent))
            .expect("insert recent event");

        let counts = persistence
            .event_counts_since(Some("2025-01-01T00:00:00Z"))
            .expect("query counts failed");

        let extracted = counts
            .iter()
            .find(|(name, _)| name == "conversation_extracted")
            .map(|(_, count)| *count)
            .unwrap_or(0);

        assert_eq!(extracted, 1);

        cleanup(&path);
    }
}
