use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};

use crate::application::ports::{ConversationRepository, EventRepository, JobRepository};
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

    pub fn find_conversation_by_id(&self, id: &str) -> Result<Option<ConversationRecord>, String> {
        let conn = self.open()?;
        conn.query_row(
            r#"
            SELECT id, user_id, source, url, title, raw_content, metadata_json,
                   captured_at, created_at, status, idempotency_key, item_ids, last_error
            FROM conversations
            WHERE id = ?1
            "#,
            [id],
            row_to_conversation,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub fn list_conversations(
        &self,
        status: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, String> {
        let conn = self.open()?;
        let limit = std::cmp::min(limit, i64::MAX as usize) as i64;
        let offset = std::cmp::min(offset, i64::MAX as usize) as i64;
        let normalized_status = normalize_status_filter(status);

        let mut out = Vec::new();
        if let Some(status) = normalized_status {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT id, user_id, source, url, title, raw_content, metadata_json,
                           captured_at, created_at, status, idempotency_key, item_ids, last_error
                    FROM conversations
                    WHERE status = ?1
                    ORDER BY captured_at DESC
                    LIMIT ?2 OFFSET ?3
                    "#,
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![status, limit, offset], row_to_conversation)
                .map_err(|e| e.to_string())?;
            for row in rows {
                out.push(row.map_err(|e| e.to_string())?);
            }
            return Ok(out);
        }

        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, user_id, source, url, title, raw_content, metadata_json,
                       captured_at, created_at, status, idempotency_key, item_ids, last_error
                FROM conversations
                ORDER BY captured_at DESC
                LIMIT ?1 OFFSET ?2
                "#,
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![limit, offset], row_to_conversation)
            .map_err(|e| e.to_string())?;
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn count_conversations(&self, status: Option<&str>) -> Result<usize, String> {
        let conn = self.open()?;
        let normalized_status = normalize_status_filter(status);
        let count: i64 = if let Some(status) = normalized_status {
            conn.query_row(
                "SELECT COUNT(*) FROM conversations WHERE status = ?1",
                [status],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?
        } else {
            conn.query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
                .map_err(|e| e.to_string())?
        };
        Ok(count.max(0) as usize)
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

    pub fn find_job_by_id(&self, id: &str) -> Result<Option<ExtractionJobRecord>, String> {
        let conn = self.open()?;
        conn.query_row(
            r#"
            SELECT id, conversation_id, mode, status, created_at, updated_at, error
            FROM extraction_jobs
            WHERE id = ?1
            "#,
            [id],
            row_to_job,
        )
        .optional()
        .map_err(|e| e.to_string())
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

    pub fn insert_or_fetch_conversation_by_idempotency(
        &self,
        record: &ConversationRecord,
    ) -> Result<ConversationRecord, String> {
        let conn = self.open()?;
        let item_ids = serde_json::to_string(&record.item_ids).map_err(|e| e.to_string())?;
        let metadata = serde_json::to_string(&record.metadata).map_err(|e| e.to_string())?;
        conn.query_row(
            r#"
            INSERT INTO conversations
              (id, user_id, source, url, title, raw_content, metadata_json,
               captured_at, created_at, status, idempotency_key, item_ids, last_error)
            VALUES
              (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(idempotency_key) DO UPDATE SET id=id
            RETURNING id, user_id, source, url, title, raw_content, metadata_json,
                      captured_at, created_at, status, idempotency_key, item_ids, last_error
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
            row_to_conversation,
        )
        .map_err(|e| e.to_string())
    }

    fn open(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        Ok(conn)
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

impl ConversationRepository for ServerPersistence {
    fn find_conversation_by_id(&self, id: &str) -> Result<Option<ConversationRecord>, String> {
        ServerPersistence::find_conversation_by_id(self, id)
    }

    fn list_conversations(
        &self,
        status: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, String> {
        ServerPersistence::list_conversations(self, status, offset, limit)
    }

    fn count_conversations(&self, status: Option<&str>) -> Result<usize, String> {
        ServerPersistence::count_conversations(self, status)
    }

    fn upsert_conversation(&self, record: &ConversationRecord) -> Result<(), String> {
        ServerPersistence::upsert_conversation(self, record)
    }

    fn insert_or_fetch_conversation_by_idempotency(
        &self,
        record: &ConversationRecord,
    ) -> Result<ConversationRecord, String> {
        ServerPersistence::insert_or_fetch_conversation_by_idempotency(self, record)
    }
}

impl JobRepository for ServerPersistence {
    fn find_job_by_id(&self, id: &str) -> Result<Option<ExtractionJobRecord>, String> {
        ServerPersistence::find_job_by_id(self, id)
    }

    fn upsert_job(&self, job: &ExtractionJobRecord) -> Result<(), String> {
        ServerPersistence::upsert_job(self, job)
    }
}

impl EventRepository for ServerPersistence {
    fn insert_event(&self, event: &EventRecord) -> Result<(), String> {
        ServerPersistence::insert_event(self, event)
    }

    fn event_counts_since(&self, since: Option<&str>) -> Result<Vec<(String, usize)>, String> {
        ServerPersistence::event_counts_since(self, since)
    }
}

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
    let metadata_raw: String = row.get(6)?;
    let item_ids_raw: String = row.get(11)?;
    let status_raw: String = row.get(9)?;

    let metadata = serde_json::from_str(&metadata_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let item_ids = serde_json::from_str(&item_ids_raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let status = conversation_status_from_db(status_raw.as_str()).map_err(|err| {
        let wrapped = std::io::Error::new(std::io::ErrorKind::InvalidData, err);
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(wrapped))
    })?;

    Ok(ConversationRecord {
        id: row.get(0)?,
        user_id: row.get(1)?,
        source: row.get(2)?,
        url: row.get(3)?,
        title: row.get(4)?,
        raw_content: row.get(5)?,
        metadata,
        captured_at: row.get(7)?,
        created_at: row.get(8)?,
        status,
        idempotency_key: row.get(10)?,
        item_ids,
        last_error: row.get(12)?,
    })
}

fn row_to_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExtractionJobRecord> {
    let mode_raw: String = row.get(2)?;
    let status_raw: String = row.get(3)?;
    let mode = extraction_mode_from_db(mode_raw.as_str()).map_err(|err| {
        let wrapped = std::io::Error::new(std::io::ErrorKind::InvalidData, err);
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(wrapped))
    })?;
    let status = job_status_from_db(status_raw.as_str()).map_err(|err| {
        let wrapped = std::io::Error::new(std::io::ErrorKind::InvalidData, err);
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(wrapped))
    })?;

    Ok(ExtractionJobRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        mode,
        status,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        error: row.get(6)?,
    })
}

fn normalize_status_filter(status: Option<&str>) -> Option<String> {
    status
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
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

fn conversation_status_from_db(raw: &str) -> Result<ConversationStatus, String> {
    match raw {
        "captured" => Ok(ConversationStatus::Captured),
        "queued" => Ok(ConversationStatus::Queued),
        "processing" => Ok(ConversationStatus::Processing),
        "processed" => Ok(ConversationStatus::Processed),
        "failed" => Ok(ConversationStatus::Failed),
        _ => Err(format!("invalid conversation status: {}", raw)),
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

fn extraction_mode_from_db(raw: &str) -> Result<ExtractionMode, String> {
    match raw {
        "auto" => Ok(ExtractionMode::Auto),
        "knowledge" => Ok(ExtractionMode::Knowledge),
        "skill" => Ok(ExtractionMode::Skill),
        "snippet" => Ok(ExtractionMode::Snippet),
        _ => Err(format!("invalid extraction mode: {}", raw)),
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

fn job_status_from_db(raw: &str) -> Result<JobStatus, String> {
    match raw {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "succeeded" => Ok(JobStatus::Succeeded),
        "failed" => Ok(JobStatus::Failed),
        _ => Err(format!("invalid job status: {}", raw)),
    }
}

fn collect_event_counts(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<(String, i64)>,
    >,
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
    use crate::models::{
        now_iso, ConversationRecord, ConversationStatus, EventRecord, ExtractionJobRecord,
        ExtractionMode, JobStatus,
    };
    use serde_json::json;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use uuid::Uuid;

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "refine-server-persistence-test-{}.db",
            Uuid::new_v4()
        ))
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

    fn build_conversation(status: ConversationStatus, idempotency_key: &str) -> ConversationRecord {
        let now = now_iso();
        ConversationRecord {
            id: Uuid::new_v4().to_string(),
            user_id: "u".to_string(),
            source: "s".to_string(),
            url: "https://example.com".to_string(),
            title: Some("title".to_string()),
            raw_content: "content".to_string(),
            metadata: json!({"k":"v"}),
            captured_at: now.clone(),
            created_at: now,
            status,
            idempotency_key: idempotency_key.to_string(),
            item_ids: vec![],
            last_error: None,
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
            .insert_event(&build_event(
                "conversation_extracted",
                "2020-01-01T00:00:00Z",
            ))
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

    #[test]
    fn conversation_queries_work_with_status_and_idempotency() {
        let path = temp_db_path();
        let persistence = ServerPersistence::new(path.clone()).expect("persistence init failed");
        let queued = build_conversation(ConversationStatus::Queued, "k1");
        let captured = build_conversation(ConversationStatus::Captured, "k2");
        persistence
            .upsert_conversation(&queued)
            .expect("upsert queued conversation");
        persistence
            .upsert_conversation(&captured)
            .expect("upsert captured conversation");

        let total = persistence
            .count_conversations(None)
            .expect("count all conversations");
        assert_eq!(total, 2);

        let queued_total = persistence
            .count_conversations(Some("queued"))
            .expect("count queued conversations");
        assert_eq!(queued_total, 1);

        let fetched = persistence
            .insert_or_fetch_conversation_by_idempotency(&queued)
            .expect("insert_or_fetch for existing key");
        assert_eq!(fetched.id, queued.id);
        assert_eq!(fetched.idempotency_key, "k1");

        let page = persistence
            .list_conversations(Some("queued"), 0, 10)
            .expect("list queued conversations");
        assert_eq!(page.len(), 1);

        cleanup(&path);
    }

    #[test]
    fn find_job_by_id_returns_saved_job() {
        let path = temp_db_path();
        let persistence = ServerPersistence::new(path.clone()).expect("persistence init failed");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: "c1".to_string(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
        };
        persistence.upsert_job(&job).expect("upsert job");

        let loaded = persistence
            .find_job_by_id(&job.id)
            .expect("find job by id")
            .expect("job should exist");
        assert_eq!(loaded.id, job.id);
        assert_eq!(loaded.status, JobStatus::Pending);

        cleanup(&path);
    }

    #[test]
    fn concurrent_inserts_same_idempotency_key_return_same_id() {
        let path = temp_db_path();
        // Pre-create the schema so both threads start from a clean, ready state.
        ServerPersistence::new(path.clone()).expect("schema init");

        let record1 = build_conversation(ConversationStatus::Queued, "concurrent-key");
        let record2 = build_conversation(ConversationStatus::Queued, "concurrent-key");

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = Arc::clone(&barrier);
        let b2 = Arc::clone(&barrier);

        let path1 = path.clone();
        let path2 = path.clone();

        let h1 = std::thread::spawn(move || {
            let p = ServerPersistence::new(path1).expect("init p1");
            b1.wait();
            p.insert_or_fetch_conversation_by_idempotency(&record1)
        });
        let h2 = std::thread::spawn(move || {
            let p = ServerPersistence::new(path2).expect("init p2");
            b2.wait();
            p.insert_or_fetch_conversation_by_idempotency(&record2)
        });

        let r1 = h1
            .join()
            .expect("thread 1 panicked")
            .expect("thread 1 error");
        let r2 = h2
            .join()
            .expect("thread 2 panicked")
            .expect("thread 2 error");

        assert_eq!(
            r1.id, r2.id,
            "concurrent inserts with same idempotency key must return the same conversation id"
        );
        assert_eq!(r1.idempotency_key, "concurrent-key");

        cleanup(&path);
    }
}
