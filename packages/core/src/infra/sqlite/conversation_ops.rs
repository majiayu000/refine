//! Conversation / extraction-job / event 三表的 SQLite 操作
//!
//! 全部在 worker 单线程内调用，假定 connection 已经过 `configure_connection`。

use rusqlite::{params, Connection, OptionalExtension};

use crate::conversation::{
    ConversationRecord, ConversationStatus, EventRecord, ExtractionJobRecord, ExtractionMode,
    JobStatus,
};
use crate::error::{InfraError, InfraResult};

pub(super) fn find_conversation_by_id(
    conn: &Connection,
    id: &str,
) -> InfraResult<Option<ConversationRecord>> {
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
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn list_conversations(
    conn: &Connection,
    status: Option<&str>,
    offset: usize,
    limit: usize,
) -> InfraResult<Vec<ConversationRecord>> {
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
            .map_err(|e| InfraError::Database(e.to_string()))?;
        let rows = stmt
            .query_map(params![status, limit, offset], row_to_conversation)
            .map_err(|e| InfraError::Database(e.to_string()))?;
        for row in rows {
            out.push(row.map_err(|e| InfraError::Database(e.to_string()))?);
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
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params![limit, offset], row_to_conversation)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    for row in rows {
        out.push(row.map_err(|e| InfraError::Database(e.to_string()))?);
    }
    Ok(out)
}

pub(super) fn count_conversations(conn: &Connection, status: Option<&str>) -> InfraResult<usize> {
    let normalized_status = normalize_status_filter(status);
    let count: i64 = if let Some(status) = normalized_status {
        conn.query_row(
            "SELECT COUNT(*) FROM conversations WHERE status = ?1",
            [status],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?
    } else {
        conn.query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
            .map_err(|e| InfraError::Database(e.to_string()))?
    };
    Ok(count.max(0) as usize)
}

pub(super) fn upsert_conversation(
    conn: &Connection,
    record: &ConversationRecord,
) -> InfraResult<()> {
    validate_conversation_transition(conn, record)?;

    let item_ids =
        serde_json::to_string(&record.item_ids).map_err(|e| InfraError::Database(e.to_string()))?;
    let metadata =
        serde_json::to_string(&record.metadata).map_err(|e| InfraError::Database(e.to_string()))?;

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
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

pub(super) fn insert_or_fetch_conversation_by_idempotency(
    conn: &Connection,
    record: &ConversationRecord,
) -> InfraResult<ConversationRecord> {
    let item_ids =
        serde_json::to_string(&record.item_ids).map_err(|e| InfraError::Database(e.to_string()))?;
    let metadata =
        serde_json::to_string(&record.metadata).map_err(|e| InfraError::Database(e.to_string()))?;
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
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn find_job_by_id(
    conn: &Connection,
    id: &str,
) -> InfraResult<Option<ExtractionJobRecord>> {
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
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn upsert_job(conn: &Connection, job: &ExtractionJobRecord) -> InfraResult<()> {
    let affected = conn
        .execute(
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
        WHERE extraction_jobs.status = excluded.status
           OR (extraction_jobs.status = 'pending' AND excluded.status = 'running')
           OR (extraction_jobs.status = 'running' AND excluded.status IN ('succeeded', 'failed'))
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
        .map_err(|e| InfraError::Database(e.to_string()))?;

    if affected == 0 {
        let existing = conn
            .query_row(
                "SELECT status FROM extraction_jobs WHERE id = ?1",
                [job.id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| InfraError::Database(e.to_string()))?
            .map(|raw| job_status_from_db(raw.as_str()))
            .transpose()
            .map_err(InfraError::Database)?;

        if let Some(existing) = existing {
            return Err(invalid_transition_error(
                "job",
                &job.id,
                job_status_to_db(&existing),
                job_status_to_db(&job.status),
            ));
        }

        return Err(invalid_transition_error(
            "job",
            &job.id,
            "<missing>",
            job_status_to_db(&job.status),
        ));
    }

    Ok(())
}

fn validate_conversation_transition(
    conn: &Connection,
    record: &ConversationRecord,
) -> InfraResult<()> {
    let existing = conn
        .query_row(
            "SELECT status FROM conversations WHERE id = ?1",
            [record.id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))?
        .map(|raw| conversation_status_from_db(raw.as_str()))
        .transpose()
        .map_err(InfraError::Database)?;

    if let Some(existing) = existing {
        if !existing.can_transition_to(&record.status) {
            return Err(invalid_transition_error(
                "conversation",
                &record.id,
                conversation_status_to_db(&existing),
                conversation_status_to_db(&record.status),
            ));
        }
    }

    Ok(())
}

fn invalid_transition_error(kind: &str, id: &str, from: &str, to: &str) -> InfraError {
    let message = format!("invalid {kind} status transition for {id}: {from} -> {to}");
    tracing::error!("{message}");
    InfraError::Database(message)
}

pub(super) fn insert_event(conn: &Connection, event: &EventRecord) -> InfraResult<()> {
    let properties = serde_json::to_string(&event.properties)
        .map_err(|e| InfraError::Database(e.to_string()))?;
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
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

pub(super) fn event_counts_since(
    conn: &Connection,
    since: Option<&str>,
) -> InfraResult<Vec<(String, usize)>> {
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
            .map_err(|e| InfraError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([since], |row| {
                let event_name: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((event_name, count))
            })
            .map_err(|e| InfraError::Database(e.to_string()))?;

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
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let rows = stmt
        .query_map([], |row| {
            let event_name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((event_name, count))
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    collect_event_counts(rows)
}

fn collect_event_counts(
    rows: rusqlite::MappedRows<
        '_,
        impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<(String, i64)>,
    >,
) -> InfraResult<Vec<(String, usize)>> {
    let mut counts = Vec::new();
    for row in rows {
        let (event_name, count) = row.map_err(|e| InfraError::Database(e.to_string()))?;
        counts.push((event_name, count.max(0) as usize));
    }
    Ok(counts)
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

#[cfg(test)]
mod tests {
    use crate::conversation::{
        now_iso, ConversationRecord, ConversationRepository, ConversationStatus, EventRecord,
        EventRepository, ExtractionJobRecord, ExtractionMode, JobRepository, JobStatus,
    };
    use crate::infra::sqlite::SqliteStore;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "refine-conversation-ops-test-{}.db",
            Uuid::new_v4()
        ))
    }

    fn cleanup(path: &PathBuf) {
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

    #[tokio::test]
    async fn event_counts_since_returns_aggregated_counts() {
        let path = temp_db_path();
        let store = SqliteStore::open(&path).expect("store init");
        let store: Arc<dyn EventRepository> = Arc::new(store);
        let now = now_iso();
        store
            .insert_event(&build_event("conversation_extracted", &now))
            .await
            .expect("insert event 1");
        store
            .insert_event(&build_event("conversation_extracted", &now))
            .await
            .expect("insert event 2");
        store
            .insert_event(&build_event("conversation_synced", &now))
            .await
            .expect("insert event 3");

        let counts = store.event_counts_since(None).await.expect("counts");
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

    #[tokio::test]
    async fn event_counts_since_filters_old_events() {
        let path = temp_db_path();
        let store = SqliteStore::open(&path).expect("store init");
        let store: Arc<dyn EventRepository> = Arc::new(store);
        store
            .insert_event(&build_event(
                "conversation_extracted",
                "2020-01-01T00:00:00Z",
            ))
            .await
            .expect("insert old");
        let recent = now_iso();
        store
            .insert_event(&build_event("conversation_extracted", &recent))
            .await
            .expect("insert recent");
        let counts = store
            .event_counts_since(Some("2025-01-01T00:00:00Z"))
            .await
            .expect("counts");
        let extracted = counts
            .iter()
            .find(|(name, _)| name == "conversation_extracted")
            .map(|(_, count)| *count)
            .unwrap_or(0);
        assert_eq!(extracted, 1);
        cleanup(&path);
    }

    #[tokio::test]
    async fn conversation_queries_work_with_status_and_idempotency() {
        let path = temp_db_path();
        let store = SqliteStore::open(&path).expect("store init");
        let store: Arc<dyn ConversationRepository> = Arc::new(store);
        let queued = build_conversation(ConversationStatus::Queued, "k1");
        let captured = build_conversation(ConversationStatus::Captured, "k2");
        store
            .upsert_conversation(&queued)
            .await
            .expect("upsert queued");
        store
            .upsert_conversation(&captured)
            .await
            .expect("upsert captured");

        let total = store.count_conversations(None).await.expect("count all");
        assert_eq!(total, 2);
        let queued_total = store
            .count_conversations(Some("queued"))
            .await
            .expect("count queued");
        assert_eq!(queued_total, 1);

        let fetched = store
            .insert_or_fetch_conversation_by_idempotency(&queued)
            .await
            .expect("fetch existing key");
        assert_eq!(fetched.id, queued.id);
        assert_eq!(fetched.idempotency_key, "k1");

        let page = store
            .list_conversations(Some("queued"), 0, 10)
            .await
            .expect("list queued");
        assert_eq!(page.len(), 1);

        cleanup(&path);
    }

    #[tokio::test]
    async fn find_job_by_id_returns_saved_job() {
        let path = temp_db_path();
        let store = SqliteStore::open(&path).expect("store init");
        let store: Arc<dyn JobRepository> = Arc::new(store);
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
        store.upsert_job(&job).await.expect("upsert job");
        let loaded = store
            .find_job_by_id(&job.id)
            .await
            .expect("find job")
            .expect("job should exist");
        assert_eq!(loaded.id, job.id);
        assert_eq!(loaded.status, JobStatus::Pending);
        cleanup(&path);
    }

    #[tokio::test]
    async fn upsert_job_rejects_invalid_status_regression() {
        let path = temp_db_path();
        let store = SqliteStore::open(&path).expect("store init");
        let store: Arc<dyn JobRepository> = Arc::new(store);
        let now = now_iso();
        let mut job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: "c1".to_string(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
        };

        store.upsert_job(&job).await.expect("insert pending job");
        job.status = JobStatus::Running;
        store.upsert_job(&job).await.expect("mark job running");
        job.status = JobStatus::Succeeded;
        store.upsert_job(&job).await.expect("mark job succeeded");

        job.status = JobStatus::Pending;
        let err = store
            .upsert_job(&job)
            .await
            .expect_err("succeeded jobs must not regress to pending");
        assert!(
            err.to_string().contains("invalid job status transition"),
            "unexpected error: {}",
            err
        );

        let loaded = store
            .find_job_by_id(&job.id)
            .await
            .expect("find job")
            .expect("job should exist");
        assert_eq!(loaded.status, JobStatus::Succeeded);
        cleanup(&path);
    }

    #[tokio::test]
    async fn concurrent_inserts_same_idempotency_key_return_same_id() {
        // Two SqliteStore instances against the same file path simulate the
        // multi-process write contention that ServerPersistence used to exercise.
        let path = temp_db_path();
        SqliteStore::open(&path).expect("schema init");

        let record1 = build_conversation(ConversationStatus::Queued, "concurrent-key");
        let record2 = build_conversation(ConversationStatus::Queued, "concurrent-key");

        let path1 = path.clone();
        let path2 = path.clone();

        let h1 = tokio::spawn(async move {
            let store = SqliteStore::open(&path1).expect("store 1");
            let store: Arc<dyn ConversationRepository> = Arc::new(store);
            store
                .insert_or_fetch_conversation_by_idempotency(&record1)
                .await
        });
        let h2 = tokio::spawn(async move {
            let store = SqliteStore::open(&path2).expect("store 2");
            let store: Arc<dyn ConversationRepository> = Arc::new(store);
            store
                .insert_or_fetch_conversation_by_idempotency(&record2)
                .await
        });

        let r1 = h1.await.expect("task 1").expect("insert 1");
        let r2 = h2.await.expect("task 2").expect("insert 2");

        assert_eq!(
            r1.id, r2.id,
            "concurrent inserts with same idempotency key must return the same conversation id"
        );
        assert_eq!(r1.idempotency_key, "concurrent-key");
        cleanup(&path);
    }
}
