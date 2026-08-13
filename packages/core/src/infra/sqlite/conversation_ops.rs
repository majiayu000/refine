//! Conversation / extraction-job / event 三表的 SQLite 操作
//!
//! 全部在 worker 单线程内调用，假定 connection 已经过 `configure_connection`。

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

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
    let item_ids =
        serde_json::to_string(&record.item_ids).map_err(|e| InfraError::Database(e.to_string()))?;
    let metadata =
        serde_json::to_string(&record.metadata).map_err(|e| InfraError::Database(e.to_string()))?;

    let affected = conn
        .execute(
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
        WHERE conversations.status = excluded.status
           OR (conversations.status = 'captured' AND excluded.status = 'queued')
           OR (conversations.status = 'queued' AND excluded.status IN ('processing', 'failed'))
           OR (conversations.status = 'processing' AND excluded.status IN ('processed', 'failed'))
           OR (conversations.status = 'failed' AND excluded.status = 'queued')
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

    if affected == 0 {
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
            return Err(invalid_transition_error(
                "conversation",
                &record.id,
                conversation_status_to_db(&existing),
                conversation_status_to_db(&record.status),
            ));
        }

        return Err(invalid_transition_error(
            "conversation",
            &record.id,
            "<missing>",
            conversation_status_to_db(&record.status),
        ));
    }

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

pub(super) fn insert_or_fetch_conversation_with_job(
    conn: &Connection,
    record: &ConversationRecord,
    job: &ExtractionJobRecord,
) -> InfraResult<(ConversationRecord, Option<ExtractionJobRecord>)> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let mut persisted = insert_or_fetch_conversation_by_idempotency(&tx, record)?;
    if persisted.status == ConversationStatus::Processed {
        tx.commit()
            .map_err(|e| InfraError::Database(e.to_string()))?;
        return Ok((persisted, None));
    }

    let existing = tx
        .query_row(
            r#"
            SELECT id, conversation_id, mode, status, created_at, updated_at, error,
                   attempt_count, lease_owner, lease_expires_at
            FROM extraction_jobs
            WHERE conversation_id = ?1 AND status IN ('pending', 'running')
            ORDER BY created_at DESC, id DESC
            LIMIT 1
            "#,
            [persisted.id.as_str()],
            row_to_job,
        )
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))?;

    let persisted_job = match existing {
        Some(existing) => Some(existing),
        None => {
            let mut initial_job = job.clone();
            initial_job.conversation_id = persisted.id.clone();
            upsert_job(&tx, &initial_job)?;
            Some(initial_job)
        }
    };
    if matches!(
        persisted.status,
        ConversationStatus::Captured | ConversationStatus::Failed
    ) {
        tx.execute(
            "UPDATE conversations SET status = 'queued', last_error = NULL WHERE id = ?1",
            [persisted.id.as_str()],
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
        persisted.status = ConversationStatus::Queued;
        persisted.last_error = None;
    }
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok((persisted, persisted_job))
}

pub(super) fn enqueue_job(
    conn: &Connection,
    job: &ExtractionJobRecord,
) -> InfraResult<ExtractionJobRecord> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let existing = tx
        .query_row(
            r#"
            SELECT id, conversation_id, mode, status, created_at, updated_at, error,
                   attempt_count, lease_owner, lease_expires_at
            FROM extraction_jobs
            WHERE conversation_id = ?1 AND status IN ('pending', 'running')
            ORDER BY created_at DESC, id DESC LIMIT 1
            "#,
            [job.conversation_id.as_str()],
            row_to_job,
        )
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if let Some(existing) = existing {
        tx.commit()
            .map_err(|e| InfraError::Database(e.to_string()))?;
        return Ok(existing);
    }
    let changed = tx
        .execute(
            r#"
            UPDATE conversations SET status = 'queued', last_error = NULL
            WHERE id = ?1 AND status IN ('captured', 'failed')
            "#,
            [job.conversation_id.as_str()],
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if changed == 0 {
        let status = tx
            .query_row(
                "SELECT status FROM conversations WHERE id = ?1",
                [job.conversation_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| InfraError::Database(e.to_string()))?;
        if status.as_deref() != Some("queued") {
            return Err(InfraError::Database(format!(
                "conversation {} cannot be queued from {}",
                job.conversation_id,
                status.as_deref().unwrap_or("<missing>")
            )));
        }
    }
    tx.execute(
        r#"
        INSERT OR IGNORE INTO extraction_jobs
          (id, conversation_id, mode, status, created_at, updated_at, error,
           attempt_count, lease_owner, lease_expires_at)
        VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, 0, NULL, NULL)
        "#,
        params![
            job.id,
            job.conversation_id,
            extraction_mode_to_db(&job.mode),
            job.created_at,
            job.updated_at,
            job.error,
        ],
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    let persisted = tx
        .query_row(
            r#"
            SELECT id, conversation_id, mode, status, created_at, updated_at, error,
                   attempt_count, lease_owner, lease_expires_at
            FROM extraction_jobs
            WHERE conversation_id = ?1 AND status IN ('pending', 'running')
            ORDER BY created_at DESC, id DESC LIMIT 1
            "#,
            [job.conversation_id.as_str()],
            row_to_job,
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(persisted)
}

pub(super) fn find_job_by_id(
    conn: &Connection,
    id: &str,
) -> InfraResult<Option<ExtractionJobRecord>> {
    conn.query_row(
        r#"
        SELECT id, conversation_id, mode, status, created_at, updated_at, error,
               attempt_count, lease_owner, lease_expires_at
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
          (id, conversation_id, mode, status, created_at, updated_at, error,
           attempt_count, lease_owner, lease_expires_at)
        VALUES
          (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(id) DO UPDATE SET
          conversation_id=excluded.conversation_id,
          mode=excluded.mode,
          status=excluded.status,
          created_at=excluded.created_at,
          updated_at=excluded.updated_at,
          error=excluded.error,
          attempt_count=excluded.attempt_count,
          lease_owner=excluded.lease_owner,
          lease_expires_at=excluded.lease_expires_at
        WHERE extraction_jobs.status = excluded.status
           OR (extraction_jobs.status = 'pending' AND excluded.status = 'running')
           OR (extraction_jobs.status = 'pending' AND excluded.status = 'failed')
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
                job.attempt_count,
                job.lease_owner,
                job.lease_expires_at,
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

pub(super) fn list_recoverable_jobs(
    conn: &Connection,
    now: &str,
    limit: usize,
) -> InfraResult<Vec<ExtractionJobRecord>> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, conversation_id, mode, status, created_at, updated_at, error,
                   attempt_count, lease_owner, lease_expires_at
            FROM extraction_jobs
            WHERE EXISTS (
                SELECT 1 FROM conversations
                WHERE conversations.id = extraction_jobs.conversation_id
                  AND conversations.status IN ('queued', 'processing', 'failed')
            ) AND (
               status = 'pending'
               OR (status = 'running' AND (
                    lease_expires_at IS NULL
                    OR julianday(lease_expires_at) <= julianday(?1)
               ))
            )
            ORDER BY created_at ASC, id ASC
            LIMIT ?2
            "#,
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(
            params![now, std::cmp::min(limit, i64::MAX as usize) as i64],
            row_to_job,
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn reconcile_processed_jobs(conn: &Connection, now: &str) -> InfraResult<usize> {
    conn.execute(
        r#"
        UPDATE extraction_jobs
        SET status = 'succeeded', updated_at = ?1, error = NULL,
            lease_owner = NULL, lease_expires_at = NULL
        WHERE status IN ('pending', 'running')
          AND EXISTS (
            SELECT 1 FROM conversations
            WHERE conversations.id = extraction_jobs.conversation_id
              AND conversations.status = 'processed'
          )
        "#,
        [now],
    )
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn claim_job(
    conn: &Connection,
    id: &str,
    owner: &str,
    now: &str,
    lease_expires_at: &str,
) -> InfraResult<Option<ExtractionJobRecord>> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let claimed = tx
        .query_row(
            r#"
            UPDATE extraction_jobs
            SET status = 'running', updated_at = ?3, error = NULL,
                attempt_count = attempt_count + 1,
                lease_owner = ?2, lease_expires_at = ?4
            WHERE id = ?1 AND EXISTS (
                SELECT 1 FROM conversations
                WHERE conversations.id = extraction_jobs.conversation_id
                  AND conversations.status IN ('queued', 'processing', 'failed')
            ) AND (
                status = 'pending'
                OR (status = 'running' AND (
                    lease_expires_at IS NULL
                    OR julianday(lease_expires_at) <= julianday(?3)
                ))
            )
            RETURNING id, conversation_id, mode, status, created_at, updated_at, error,
                      attempt_count, lease_owner, lease_expires_at
            "#,
            params![id, owner, now, lease_expires_at],
            row_to_job,
        )
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if let Some(job) = &claimed {
        tx.execute(
            r#"
            UPDATE conversations
            SET status = 'processing', last_error = NULL
            WHERE id = ?1 AND status IN ('queued', 'processing', 'failed')
            "#,
            [job.conversation_id.as_str()],
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    }
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(claimed)
}

pub(super) fn renew_job_lease(
    conn: &Connection,
    id: &str,
    owner: &str,
    now: &str,
    lease_expires_at: &str,
) -> InfraResult<bool> {
    conn.execute(
        r#"
        UPDATE extraction_jobs
        SET updated_at = ?3, lease_expires_at = ?4
        WHERE id = ?1 AND status = 'running' AND lease_owner = ?2
        "#,
        params![id, owner, now, lease_expires_at],
    )
    .map(|affected| affected == 1)
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn finish_job_claim(
    conn: &Connection,
    id: &str,
    owner: &str,
    status: JobStatus,
    item_ids: &[String],
    error: Option<&str>,
    now: &str,
) -> InfraResult<bool> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let finished = finish_job_claim_in_transaction(&tx, id, owner, status, item_ids, error, now)?;
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(finished)
}

pub(super) fn finish_job_claim_in_transaction(
    conn: &Connection,
    id: &str,
    owner: &str,
    status: JobStatus,
    item_ids: &[String],
    error: Option<&str>,
    now: &str,
) -> InfraResult<bool> {
    if !matches!(status, JobStatus::Succeeded | JobStatus::Failed) {
        return Err(InfraError::Database(
            "claimed job can only finish as succeeded or failed".to_string(),
        ));
    }
    let conversation_id = conn
        .query_row(
            r#"
            UPDATE extraction_jobs
            SET status = ?3, updated_at = ?4, error = ?5,
                lease_owner = NULL, lease_expires_at = NULL
            WHERE id = ?1 AND status = 'running' AND lease_owner = ?2
            RETURNING conversation_id
            "#,
            params![id, owner, job_status_to_db(&status), now, error],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let Some(conversation_id) = conversation_id else {
        return Ok(false);
    };

    let item_ids =
        serde_json::to_string(item_ids).map_err(|e| InfraError::Database(e.to_string()))?;
    match status {
        JobStatus::Succeeded => {
            let changed = conn
                .execute(
                    r#"
                UPDATE conversations
                SET status = 'processed', item_ids = ?2, last_error = NULL
                WHERE id = ?1 AND status IN ('queued', 'processing')
                "#,
                    params![conversation_id, item_ids],
                )
                .map_err(|e| InfraError::Database(e.to_string()))?;
            if changed != 1 {
                return Err(InfraError::Database(format!(
                    "claimed job {id} could not finish its conversation successfully"
                )));
            }
        }
        JobStatus::Failed => {
            let changed = conn
                .execute(
                    r#"
                UPDATE conversations
                SET status = 'failed', last_error = ?2
                WHERE id = ?1 AND status IN ('queued', 'processing')
                "#,
                    params![conversation_id, error],
                )
                .map_err(|e| InfraError::Database(e.to_string()))?;
            if changed != 1 {
                return Err(InfraError::Database(format!(
                    "claimed job {id} could not fail its conversation"
                )));
            }
        }
        _ => unreachable!(),
    }
    Ok(true)
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
        attempt_count: row.get(7)?,
        lease_owner: row.get(8)?,
        lease_expires_at: row.get(9)?,
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
    use tokio::sync::Barrier;
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
        let sqlite_store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversations: Arc<dyn ConversationRepository> = sqlite_store.clone();
        let store: Arc<dyn JobRepository> = sqlite_store;
        let now = now_iso();
        let conversation = build_conversation(ConversationStatus::Queued, "job-parent");
        conversations
            .upsert_conversation(&conversation)
            .await
            .expect("insert parent conversation");
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
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
        let sqlite_store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversations: Arc<dyn ConversationRepository> = sqlite_store.clone();
        let store: Arc<dyn JobRepository> = sqlite_store;
        let now = now_iso();
        let conversation = build_conversation(ConversationStatus::Queued, "job-transition-parent");
        conversations
            .upsert_conversation(&conversation)
            .await
            .expect("insert parent conversation");
        let mut job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
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
    async fn conversation_and_initial_job_are_created_atomically_and_idempotently() {
        let path = temp_db_path();
        let store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversation = build_conversation(ConversationStatus::Queued, "atomic-job-key");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
        };

        let (first_conversation, first_job) = store
            .insert_or_fetch_conversation_with_job(&conversation, &job)
            .await
            .expect("insert conversation and job");
        let mut duplicate = conversation.clone();
        duplicate.id = Uuid::new_v4().to_string();
        let mut duplicate_job = job.clone();
        duplicate_job.id = Uuid::new_v4().to_string();
        duplicate_job.conversation_id = duplicate.id.clone();
        let (second_conversation, second_job) = store
            .insert_or_fetch_conversation_with_job(&duplicate, &duplicate_job)
            .await
            .expect("fetch conversation and active job");

        assert_eq!(first_conversation.id, second_conversation.id);
        assert_eq!(
            first_job.expect("first job").id,
            second_job.expect("second job").id
        );
        cleanup(&path);
    }

    #[tokio::test]
    async fn processed_idempotent_replay_does_not_restart_terminal_job() {
        let path = temp_db_path();
        let store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversation = build_conversation(ConversationStatus::Queued, "processed-replay-key");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now.clone(),
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
        };
        let (_, initial_job) = store
            .insert_or_fetch_conversation_with_job(&conversation, &job)
            .await
            .expect("insert conversation and job");
        let claimed = store
            .claim_job(
                &initial_job.expect("initial job").id,
                "worker",
                &now,
                "2099-01-01T00:00:00Z",
            )
            .await
            .expect("claim job")
            .expect("claimed job");
        store
            .finish_job_claim(
                &claimed.id,
                "worker",
                JobStatus::Succeeded,
                &[],
                None,
                &now_iso(),
            )
            .await
            .expect("finish job");

        let mut replay = conversation.clone();
        replay.id = Uuid::new_v4().to_string();
        let mut replay_job = job;
        replay_job.id = Uuid::new_v4().to_string();
        replay_job.conversation_id = replay.id.clone();
        let (persisted, replayed_job) = store
            .insert_or_fetch_conversation_with_job(&replay, &replay_job)
            .await
            .expect("replay processed conversation");
        assert_eq!(persisted.status, ConversationStatus::Processed);
        assert!(replayed_job.is_none());
        cleanup(&path);
    }

    #[tokio::test]
    async fn only_one_worker_can_claim_a_pending_job() {
        let path = temp_db_path();
        let store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversation = build_conversation(ConversationStatus::Queued, "claim-parent");
        store
            .upsert_conversation(&conversation)
            .await
            .expect("insert conversation");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now.clone(),
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
        };
        store.upsert_job(&job).await.expect("insert job");
        let expires = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();

        let first = store
            .claim_job(&job.id, "worker-a", &now, &expires)
            .await
            .expect("claim a");
        let second = store
            .claim_job(&job.id, "worker-b", &now, &expires)
            .await
            .expect("claim b");
        assert!(first.is_some());
        assert!(second.is_none());
        assert_eq!(first.expect("claimed").attempt_count, 1);
        cleanup(&path);
    }

    #[tokio::test]
    async fn stale_lease_is_recoverable_and_old_owner_cannot_finish() {
        let path = temp_db_path();
        let store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let conversation = build_conversation(ConversationStatus::Queued, "stale-parent");
        store
            .upsert_conversation(&conversation)
            .await
            .expect("insert conversation");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Knowledge,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now.clone(),
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
        };
        store.upsert_job(&job).await.expect("insert job");
        let expired = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        store
            .claim_job(&job.id, "crashed-worker", &now, &expired)
            .await
            .expect("initial claim")
            .expect("claimed");

        let recoverable = store
            .list_recoverable_jobs(&now_iso(), 10)
            .await
            .expect("list recoverable");
        assert_eq!(recoverable.len(), 1);
        let expires = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
        let reclaimed = store
            .claim_job(&job.id, "recovery-worker", &now_iso(), &expires)
            .await
            .expect("reclaim")
            .expect("reclaimed");
        assert_eq!(reclaimed.attempt_count, 2);

        assert!(!store
            .finish_job_claim(
                &job.id,
                "crashed-worker",
                JobStatus::Failed,
                &[],
                Some("late failure"),
                &now_iso(),
            )
            .await
            .expect("stale finish"));
        assert!(store
            .finish_job_claim(
                &job.id,
                "recovery-worker",
                JobStatus::Succeeded,
                &["item-1".to_string()],
                None,
                &now_iso(),
            )
            .await
            .expect("current finish"));
        let conversation = store
            .find_conversation_by_id(&conversation.id)
            .await
            .expect("load conversation")
            .expect("conversation exists");
        assert_eq!(conversation.status, ConversationStatus::Processed);
        assert_eq!(conversation.item_ids, vec!["item-1"]);
        cleanup(&path);
    }

    #[tokio::test]
    async fn processed_parent_is_not_recovered_or_reclaimed() {
        let path = temp_db_path();
        let store = Arc::new(SqliteStore::open(&path).expect("store init"));
        let mut conversation =
            build_conversation(ConversationStatus::Queued, "processed-running-parent");
        store
            .upsert_conversation(&conversation)
            .await
            .expect("insert conversation");
        let now = now_iso();
        let job = ExtractionJobRecord {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation.id.clone(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Pending,
            created_at: now.clone(),
            updated_at: now.clone(),
            error: None,
            attempt_count: 0,
            lease_owner: None,
            lease_expires_at: None,
        };
        store.upsert_job(&job).await.expect("insert job");
        store
            .claim_job(&job.id, "legacy-worker", &now, "2020-01-01T00:00:00Z")
            .await
            .expect("claim")
            .expect("claimed");
        conversation.status = ConversationStatus::Processing;
        store
            .upsert_conversation(&conversation)
            .await
            .expect("processing");
        conversation.status = ConversationStatus::Processed;
        conversation.item_ids = vec!["old-item".to_string()];
        store
            .upsert_conversation(&conversation)
            .await
            .expect("processed");

        assert!(store
            .list_recoverable_jobs(&now_iso(), 10)
            .await
            .expect("recoverable")
            .is_empty());
        assert!(store
            .claim_job(&job.id, "new-worker", &now_iso(), "2099-01-01T00:00:00Z",)
            .await
            .expect("claim processed")
            .is_none());
        assert_eq!(
            store
                .reconcile_processed_jobs(&now_iso())
                .await
                .expect("reconcile processed"),
            1
        );
        let reconciled = store
            .find_job_by_id(&job.id)
            .await
            .expect("load reconciled job")
            .expect("job exists");
        assert_eq!(reconciled.status, JobStatus::Succeeded);
        let parent = store
            .find_conversation_by_id(&conversation.id)
            .await
            .expect("load parent")
            .expect("parent exists");
        assert_eq!(parent.item_ids, vec!["old-item"]);
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
        let barrier = Arc::new(Barrier::new(2));

        let barrier1 = barrier.clone();
        let h1 = tokio::spawn(async move {
            let store = SqliteStore::open(&path1).expect("store 1");
            let store: Arc<dyn ConversationRepository> = Arc::new(store);
            barrier1.wait().await;
            store
                .insert_or_fetch_conversation_by_idempotency(&record1)
                .await
        });
        let barrier2 = barrier;
        let h2 = tokio::spawn(async move {
            let store = SqliteStore::open(&path2).expect("store 2");
            let store: Arc<dyn ConversationRepository> = Arc::new(store);
            barrier2.wait().await;
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
