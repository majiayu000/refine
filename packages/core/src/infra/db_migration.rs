use fs2::FileExt;
use rusqlite::{backup::Backup, backup::StepResult, params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::paths::stale_db_candidates;

/// Result of a `migrate_stale_dbs` run.
pub enum MigrationReport {
    /// No legacy files were found; nothing was done.
    NoOp,
    /// One or more legacy databases were merged into the target.
    Migrated {
        sources: Vec<PathBuf>,
        rows_copied: usize,
    },
}

struct MigrationSnapshot {
    path: PathBuf,
    remove_on_drop: bool,
}

struct MigrationState {
    signature: String,
    content_hash: String,
}

impl Drop for MigrationSnapshot {
    fn drop(&mut self) {
        if self.remove_on_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Copies rows from any legacy databases into `target`.
///
/// Legacy files deliberately remain at their original paths: an older process
/// may still hold a writable connection and append rows after this pass. Future
/// starts safely reconcile those rows through table-specific upserts. Source
/// deletions are not propagated because legacy schemas have no tombstones. On
/// failure, all writes for the failing source are rolled back.
pub fn migrate_stale_dbs(target: &Path) -> Result<MigrationReport, String> {
    let candidates = stale_db_candidates(target);
    if candidates.is_empty() {
        return Ok(MigrationReport::NoOp);
    }

    let conn = Connection::open(target)
        .map_err(|e| format!("failed to open target DB {}: {}", target.display(), e))?;

    crate::infra::configure_sqlite_connection(&conn)
        .map_err(|e| format!("failed to configure target connection: {}", e))?;

    crate::infra::prepare_sqlite_db(&conn)
        .map_err(|e| format!("failed to initialise target schema: {}", e))?;
    prepare_migration_state(&conn)?;
    prepare_import_ledger(&conn)?;

    let mut sources = Vec::new();
    let mut total_rows = 0usize;

    for candidate in &candidates {
        let signature_before = source_signature(candidate)?;
        let previous_state = migration_state(&conn, candidate)?;
        if !force_reconcile()
            && cheap_signature_is_authoritative()
            && previous_state
                .as_ref()
                .map(|state| state.signature.as_str())
                == Some(signature_before.as_str())
        {
            continue;
        }
        let bak_path = with_suffix(candidate, ".pre-migration.bak");
        let snapshot = match create_consistent_backup(candidate, &bak_path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                preserve_forensic_bundle(candidate)?;
                return Err(error);
            }
        };
        let content_hash = hash_file(&snapshot.path)?;
        let signature_after_snapshot = source_signature(candidate)?;
        if !force_reconcile()
            && signature_after_snapshot == signature_before
            && previous_state
                .as_ref()
                .map(|state| state.content_hash.as_str())
                == Some(content_hash.as_str())
        {
            save_migration_state(&conn, candidate, &signature_after_snapshot, &content_hash)?;
            continue;
        }

        // Import the exact verified snapshot. Concurrent rows committed after
        // the snapshot remain discoverable at the legacy source path and are
        // reconciled on a later start.
        let conn = Connection::open(target)
            .map_err(|e| format!("failed to reopen target DB {}: {}", target.display(), e))?;
        crate::infra::configure_sqlite_connection(&conn)
            .map_err(|e| format!("failed to configure target connection: {}", e))?;

        let attach_sql = format!(
            "ATTACH DATABASE '{}' AS refine_migration_src",
            snapshot.path.to_string_lossy().replace('\'', "''")
        );
        if let Err(e) = conn.execute_batch(&attach_sql) {
            return Err(format!("failed to attach {}: {}", candidate.display(), e));
        }

        let result: Result<usize, String> = (|| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("failed to start migration transaction: {e}"))?;
            let rows = copy_all_tables(&tx, "refine_migration_src", candidate)?;
            let signature_after = source_signature(candidate)?;
            if signature_after != signature_before {
                return Err(format!(
                    "legacy DB {} changed while its migration snapshot was imported; retry migration",
                    candidate.display()
                ));
            }
            save_migration_state(&tx, candidate, &signature_after, &content_hash)?;
            tx.commit()
                .map_err(|e| format!("failed to commit migration transaction: {e}"))?;
            Ok(rows)
        })();
        drop(conn);
        let rows =
            result.map_err(|e| format!("migration of {} failed: {}", candidate.display(), e))?;

        sources.push(candidate.clone());
        total_rows += rows;
    }

    if sources.is_empty() {
        Ok(MigrationReport::NoOp)
    } else {
        Ok(MigrationReport::Migrated {
            sources,
            rows_copied: total_rows,
        })
    }
}

fn prepare_migration_state(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS refine_legacy_migration_state (
            source_path TEXT PRIMARY KEY,
            signature TEXT NOT NULL,
            content_hash TEXT NOT NULL DEFAULT '',
            migrated_at TEXT NOT NULL
        )",
    )
    .map_err(|e| format!("failed to prepare legacy migration state: {e}"))?;
    let columns = table_columns(conn, "main", "refine_legacy_migration_state")?;
    if !columns.iter().any(|column| column == "content_hash") {
        conn.execute_batch(
            "ALTER TABLE refine_legacy_migration_state
             ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
        )
        .map_err(|e| format!("failed to upgrade legacy migration state: {e}"))?;
    }
    Ok(())
}

fn prepare_import_ledger(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS refine_legacy_imported_rows (
            source_path TEXT NOT NULL,
            table_name TEXT NOT NULL,
            source_id TEXT NOT NULL,
            canonical_id TEXT NOT NULL,
            PRIMARY KEY (source_path, table_name, source_id)
        )",
    )
    .map_err(|e| format!("failed to prepare legacy import ledger: {e}"))
}

fn migration_state(conn: &Connection, source: &Path) -> Result<Option<MigrationState>, String> {
    conn.query_row(
        "SELECT signature, content_hash FROM refine_legacy_migration_state WHERE source_path=?1",
        [source.to_string_lossy().as_ref()],
        |row| {
            Ok(MigrationState {
                signature: row.get(0)?,
                content_hash: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("failed to read legacy migration state: {e}"))
}

fn save_migration_state(
    conn: &Connection,
    source: &Path,
    signature: &str,
    content_hash: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO refine_legacy_migration_state
           (source_path, signature, content_hash, migrated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source_path) DO UPDATE SET
           signature=excluded.signature,
           content_hash=excluded.content_hash,
           migrated_at=excluded.migrated_at",
        params![
            source.to_string_lossy().as_ref(),
            signature,
            content_hash,
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("failed to save legacy migration state: {e}"))
}

fn source_signature(source: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for suffix in ["", "-wal"] {
        let path = with_suffix(source, suffix);
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                let modified = metadata
                    .modified()
                    .map_err(|e| format!("failed to read mtime for {}: {e}", path.display()))?
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| format!("invalid mtime for {}: {e}", path.display()))?;
                #[cfg(unix)]
                let identity = {
                    use std::os::unix::fs::MetadataExt;
                    format!(
                        ":{}:{}:{}",
                        metadata.ino(),
                        metadata.ctime(),
                        metadata.ctime_nsec()
                    )
                };
                #[cfg(not(unix))]
                let identity = String::new();
                parts.push(format!(
                    "{suffix}:{}:{}{}",
                    metadata.len(),
                    modified.as_nanos(),
                    identity
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parts.push(format!("{suffix}:missing"));
            }
            Err(error) => return Err(format!("failed to inspect {}: {error}", path.display())),
        }
    }
    Ok(parts.join("|"))
}

#[cfg(unix)]
fn cheap_signature_is_authoritative() -> bool {
    true
}

#[cfg(not(unix))]
fn cheap_signature_is_authoritative() -> bool {
    false
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| {
        format!(
            "failed to open snapshot {} for hashing: {e}",
            path.display()
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("failed to hash snapshot {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn force_reconcile() -> bool {
    matches!(
        std::env::var("REFINE_FORCE_LEGACY_RECONCILE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn create_consistent_backup(
    source: &Path,
    destination: &Path,
) -> Result<MigrationSnapshot, String> {
    let source_conn = Connection::open(source)
        .map_err(|e| format!("failed to open legacy DB {}: {}", source.display(), e))?;
    source_conn
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| format!("failed to configure legacy DB {}: {}", source.display(), e))?;
    let unique = uuid::Uuid::new_v4();
    let temporary = with_suffix(destination, &format!(".tmp-{unique}"));
    let result = (|| {
        let mut destination_conn = Connection::open(&temporary).map_err(|e| {
            format!(
                "failed to create temporary backup {} for {}: {}",
                temporary.display(),
                source.display(),
                e
            )
        })?;
        {
            let backup = Backup::new(&source_conn, &mut destination_conn)
                .map_err(|e| format!("failed to start backup of {}: {}", source.display(), e))?;
            run_backup_with_deadline(&backup, source, backup_stall_timeout())?;
        }
        let integrity: String = destination_conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("failed to verify backup of {}: {}", source.display(), e))?;
        if integrity != "ok" {
            return Err(format!(
                "backup integrity check failed for {}: {}",
                source.display(),
                integrity
            ));
        }
        drop(destination_conn);

        publish_first_backup(&temporary, destination)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn publish_first_backup(temporary: &Path, destination: &Path) -> Result<MigrationSnapshot, String> {
    let lock_path = with_suffix(destination, ".publish.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| {
            format!(
                "failed to open backup publish lock {}: {e}",
                lock_path.display()
            )
        })?;
    lock_file.lock_exclusive().map_err(|e| {
        format!(
            "failed to lock backup publication {}: {e}",
            lock_path.display()
        )
    })?;

    let result = if destination.exists() {
        Ok(MigrationSnapshot {
            path: temporary.to_path_buf(),
            remove_on_drop: true,
        })
    } else {
        // Both paths are in the same directory, so rename publishes the fully
        // verified SQLite snapshot atomically without requiring hard links.
        std::fs::rename(temporary, destination).map_err(|e| {
            format!(
                "failed to publish backup {} for {}: {}",
                destination.display(),
                temporary.display(),
                e
            )
        })?;
        Ok(MigrationSnapshot {
            path: destination.to_path_buf(),
            remove_on_drop: false,
        })
    };
    let unlock_result = FileExt::unlock(&lock_file).map_err(|e| {
        format!(
            "failed to unlock backup publication {}: {e}",
            lock_path.display()
        )
    });
    match (result, unlock_result) {
        (Ok(snapshot), Ok(())) => Ok(snapshot),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn run_backup_with_deadline(
    backup: &Backup<'_, '_>,
    source: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut last_progress = Instant::now();
    loop {
        let step = backup
            .step(128)
            .map_err(|e| format!("failed to backup {}: {}", source.display(), e))?;
        match step {
            StepResult::Done => return Ok(()),
            StepResult::More => last_progress = Instant::now(),
            StepResult::Busy | StepResult::Locked => {
                if last_progress.elapsed() >= timeout {
                    return Err(format!(
                        "backup made no progress for {}ms while reading {}",
                        timeout.as_millis(),
                        source.display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {}
        }
    }
}

fn backup_stall_timeout() -> Duration {
    std::env::var("REFINE_LEGACY_BACKUP_STALL_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(30))
}

fn forensic_bundle_path(source: &Path) -> Result<PathBuf, String> {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "legacy.db".to_string());
    let signature = source_signature(source)?;
    let digest = signature_digest(source, &signature);
    Ok(parent.join(format!("{name}.pre-migration.forensic-{}", &digest[..16])))
}

fn signature_digest(source: &Path, signature: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.to_string_lossy().as_bytes());
    hasher.update([0]);
    hasher.update(signature.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn preserve_forensic_bundle(source: &Path) -> Result<(), String> {
    let bundle = forensic_bundle_path(source)?;
    if bundle.exists() {
        return Ok(());
    }
    let parent = bundle.parent().unwrap_or_else(|| Path::new("."));
    let temp_bundle = parent.join(format!(
        ".{}.tmp-{}",
        bundle
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "legacy-forensic".into()),
        uuid::Uuid::new_v4()
    ));
    let result: Result<(), String> = (|| {
        std::fs::create_dir(&temp_bundle).map_err(|e| {
            format!(
                "failed to create forensic bundle {}: {}",
                temp_bundle.display(),
                e
            )
        })?;
        let mut copied = Vec::new();
        for suffix in ["", "-wal", "-shm"] {
            let original = with_suffix(source, suffix);
            if !original.exists() {
                continue;
            }
            let name = if suffix.is_empty() {
                "main.db".to_string()
            } else {
                format!("main.db{suffix}")
            };
            let forensic = temp_bundle.join(&name);
            std::fs::copy(&original, &forensic).map_err(|e| {
                format!(
                    "failed to preserve forensic copy {} as {}: {}",
                    original.display(),
                    forensic.display(),
                    e
                )
            })?;
            copied.push(name);
        }
        std::fs::write(
            temp_bundle.join("MANIFEST.txt"),
            format!(
                "source={}\ncomplete=false\nfiles={}\n",
                source.display(),
                copied.join(",")
            ),
        )
        .map_err(|e| {
            format!(
                "failed to write forensic manifest {}: {}",
                temp_bundle.display(),
                e
            )
        })?;
        std::fs::rename(&temp_bundle, &bundle).map_err(|e| {
            format!(
                "failed to publish forensic bundle {}: {}",
                bundle.display(),
                e
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temp_bundle);
    }
    if bundle.exists() {
        return Ok(());
    }
    result
}

fn copy_all_tables(conn: &Connection, legacy_alias: &str, source: &Path) -> Result<usize, String> {
    let target_tables = list_base_tables(conn, "main")?;
    let legacy_tables = list_base_tables(conn, legacy_alias)?;
    let ordered_tables = migration_copy_order(&legacy_tables);

    create_identity_maps(conn, legacy_alias, &legacy_tables)?;

    let mut total = 0usize;
    for table in &ordered_tables {
        if !target_tables.contains(table) {
            continue;
        }
        total += copy_table(conn, table, legacy_alias, source)?;
    }
    Ok(total)
}

fn migration_copy_order(legacy_tables: &[String]) -> Vec<String> {
    let preferred = [
        "documents",
        "items",
        "conversations",
        "extraction_jobs",
        "events",
    ];
    let mut ordered = Vec::with_capacity(preferred.len());
    for table in preferred {
        if legacy_tables.iter().any(|existing| existing == table) {
            ordered.push(table.to_string());
        }
    }
    ordered
}

fn create_identity_maps(
    conn: &Connection,
    legacy_alias: &str,
    legacy_tables: &[String],
) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS refine_document_id_map (
            source_id TEXT PRIMARY KEY, canonical_id TEXT NOT NULL
         );
         DELETE FROM refine_document_id_map;
         CREATE TEMP TABLE IF NOT EXISTS refine_conversation_id_map (
            source_id TEXT PRIMARY KEY, canonical_id TEXT NOT NULL
         );
         DELETE FROM refine_conversation_id_map;",
    )
    .map_err(|e| format!("failed to prepare legacy identity maps: {e}"))?;

    if legacy_tables.iter().any(|table| table == "documents") {
        conn.execute_batch(&format!(
            "INSERT INTO refine_document_id_map (source_id, canonical_id)
             SELECT src.id, COALESCE(
               target.id,
               (SELECT first.id FROM {legacy_alias}.documents AS first
                WHERE first.url = src.url ORDER BY first.rowid LIMIT 1)
             )
             FROM {legacy_alias}.documents AS src
             LEFT JOIN main.documents AS target ON target.url = src.url"
        ))
        .map_err(|e| format!("failed to map legacy document identities: {e}"))?;
    }
    if legacy_tables.iter().any(|table| table == "conversations") {
        conn.execute_batch(&format!(
            "INSERT INTO refine_conversation_id_map (source_id, canonical_id)
             SELECT src.id, COALESCE(
               target.id,
               (SELECT first.id FROM {legacy_alias}.conversations AS first
                WHERE first.idempotency_key = src.idempotency_key
                ORDER BY first.rowid LIMIT 1)
             )
             FROM {legacy_alias}.conversations AS src
             LEFT JOIN main.conversations AS target
               ON target.idempotency_key = src.idempotency_key"
        ))
        .map_err(|e| format!("failed to map legacy conversation identities: {e}"))?;
    }
    Ok(())
}

fn list_base_tables(conn: &Connection, db_alias: &str) -> Result<Vec<String>, String> {
    let sql = format!(
        "SELECT name FROM {db_alias}.sqlite_master \
         WHERE type='table' \
           AND name NOT LIKE '%_fts%' \
           AND name NOT LIKE 'sqlite_%'"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .filter(|name| is_safe_identifier(name))
        .collect();
    Ok(names)
}

fn table_columns(conn: &Connection, db_alias: &str, table: &str) -> Result<Vec<String>, String> {
    let sql = format!("PRAGMA {db_alias}.table_info({table})");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(cols)
}

fn copy_table(
    conn: &Connection,
    table: &str,
    legacy_alias: &str,
    source: &Path,
) -> Result<usize, String> {
    let target_cols = table_columns(conn, "main", table)?;
    let legacy_cols = table_columns(conn, legacy_alias, table)?;
    let common: Vec<String> = legacy_cols
        .into_iter()
        .filter(|c| target_cols.contains(c))
        .collect();
    if common.is_empty() {
        return Ok(0);
    }
    let col_list = common.join(", ");
    let select_list = common
        .iter()
        .map(|column| match (table, column.as_str()) {
            ("documents", "id") => "doc_map.canonical_id".to_string(),
            ("items", "document_id") => {
                "COALESCE(doc_map.canonical_id, src.document_id)".to_string()
            }
            ("conversations", "id") => "conv_map.canonical_id".to_string(),
            ("extraction_jobs", "conversation_id") => {
                "COALESCE(conv_map.canonical_id, src.conversation_id)".to_string()
            }
            _ => format!("src.{column}"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let joins = match table {
        "documents" => "JOIN refine_document_id_map AS doc_map ON doc_map.source_id = src.id",
        "items" if common.iter().any(|column| column == "document_id") => {
            "LEFT JOIN refine_document_id_map AS doc_map ON doc_map.source_id = src.document_id"
        }
        "conversations" => {
            "JOIN refine_conversation_id_map AS conv_map ON conv_map.source_id = src.id"
        }
        "extraction_jobs" if common.iter().any(|column| column == "conversation_id") => {
            "LEFT JOIN refine_conversation_id_map AS conv_map ON conv_map.source_id = src.conversation_id"
        }
        _ => "",
    };
    let update_columns: Vec<&String> = common
        .iter()
        .filter(|column| column.as_str() != "id")
        .collect();
    let assignments = update_columns
        .iter()
        .map(|column| format!("{column}=excluded.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    let conflict = match table {
        "documents" if common.iter().any(|c| c == "updated_at") => document_conflict(&common),
        "items" | "extraction_jobs" if common.iter().any(|c| c == "updated_at") => {
            format!(
                "ON CONFLICT(id) DO UPDATE SET {assignments} \
                 WHERE julianday(excluded.updated_at) > julianday({table}.updated_at) \
                    OR (julianday(excluded.updated_at) = julianday({table}.updated_at) \
                        AND excluded.updated_at > {table}.updated_at)"
            )
        }
        "conversations" => format!(
            "ON CONFLICT(id) DO UPDATE SET {assignments} WHERE \
             (conversations.status = excluded.status) OR \
             (conversations.status = 'captured' AND excluded.status IN ('queued','processing','processed','failed')) OR \
             (conversations.status = 'queued' AND excluded.status IN ('processing','processed','failed')) OR \
             (conversations.status = 'processing' AND excluded.status IN ('processed','failed')) OR \
             (conversations.status = 'failed' AND excluded.status = 'queued')"
        ),
        _ => "ON CONFLICT(id) DO NOTHING".to_string(),
    };
    let order_by = match table {
        "documents" => document_source_order(&common),
        _ => String::new(),
    };
    let sql = format!(
        "INSERT INTO {table} ({col_list}) \
         SELECT {select_list} FROM {legacy_alias}.{table} AS src {joins} \
         WHERE NOT EXISTS ( \
           SELECT 1 FROM main.refine_legacy_imported_rows AS imported \
           WHERE imported.source_path=?1 \
             AND imported.table_name='{table}' \
             AND imported.source_id=src.id \
             AND NOT EXISTS ( \
               SELECT 1 FROM main.{table} AS current \
               WHERE current.id=imported.canonical_id \
             ) \
         ) {order_by} {conflict}"
    );
    let source_path = source.to_string_lossy();
    let copied = conn
        .execute(&sql, [source_path.as_ref()])
        .map_err(|e| format!("failed to copy table {table}: {e}"))?;
    record_imported_rows(conn, table, legacy_alias, source_path.as_ref())?;
    Ok(copied)
}

fn record_imported_rows(
    conn: &Connection,
    table: &str,
    legacy_alias: &str,
    source_path: &str,
) -> Result<(), String> {
    let (canonical_id, joins) = match table {
        "documents" => (
            "doc_map.canonical_id",
            "JOIN refine_document_id_map AS doc_map ON doc_map.source_id=src.id",
        ),
        "conversations" => (
            "conv_map.canonical_id",
            "JOIN refine_conversation_id_map AS conv_map ON conv_map.source_id=src.id",
        ),
        _ => ("src.id", ""),
    };
    let sql = format!(
        "INSERT INTO main.refine_legacy_imported_rows \
           (source_path, table_name, source_id, canonical_id) \
         SELECT ?1, '{table}', src.id, {canonical_id} \
         FROM {legacy_alias}.{table} AS src {joins} \
         JOIN main.{table} AS current ON current.id={canonical_id} \
         ON CONFLICT(source_path, table_name, source_id) DO UPDATE SET \
           canonical_id=excluded.canonical_id"
    );
    conn.execute(&sql, [source_path])
        .map(|_| ())
        .map_err(|e| format!("failed to record imported rows for table {table}: {e}"))
}

fn document_source_order(common: &[String]) -> String {
    let has_column = |name: &str| common.iter().any(|column| column == name);
    let mut terms = Vec::new();

    if has_column("updated_at") {
        terms.push("julianday(src.updated_at)");
        terms.push("src.updated_at");
    }
    if has_column("captured_at") {
        terms.push("julianday(src.captured_at)");
        terms.push("src.captured_at");
    }
    terms.push("src.rowid");

    format!("ORDER BY {}", terms.join(", "))
}

fn document_conflict(common: &[String]) -> String {
    let has_column = |name: &str| common.iter().any(|column| column == name);
    let mut assignments = Vec::new();

    if has_column("title") {
        assignments.push(
            "title=CASE \
               WHEN TRIM(COALESCE(excluded.title, '')) = '' THEN documents.title \
               ELSE excluded.title END"
                .to_string(),
        );
    }
    if has_column("raw_content") {
        assignments.push("raw_content=excluded.raw_content".to_string());
    }
    if has_column("source_version") {
        assignments.push("source_version=excluded.source_version".to_string());
    }
    if has_column("captured_at") {
        assignments.push("captured_at=excluded.captured_at".to_string());
    }
    assignments.push("updated_at=excluded.updated_at".to_string());

    let freshness = document_freshness_predicate(common);
    format!(
        "ON CONFLICT(id) DO UPDATE SET {} WHERE {freshness}",
        assignments.join(", ")
    )
}

fn document_freshness_predicate(common: &[String]) -> String {
    let has_column = |name: &str| common.iter().any(|column| column == name);
    let mut predicates = vec![
        "julianday(excluded.updated_at) > julianday(documents.updated_at)".to_string(),
        "(julianday(excluded.updated_at) = julianday(documents.updated_at) \
          AND excluded.updated_at > documents.updated_at)"
            .to_string(),
    ];

    if has_column("captured_at") {
        predicates.extend([
            "(julianday(excluded.updated_at) = julianday(documents.updated_at) \
              AND excluded.updated_at = documents.updated_at \
              AND julianday(excluded.captured_at) > julianday(documents.captured_at))"
                .to_string(),
            "(julianday(excluded.updated_at) = julianday(documents.updated_at) \
              AND excluded.updated_at = documents.updated_at \
              AND julianday(excluded.captured_at) = julianday(documents.captured_at) \
              AND excluded.captured_at > documents.captured_at)"
                .to_string(),
            "(julianday(excluded.updated_at) = julianday(documents.updated_at) \
              AND excluded.updated_at = documents.updated_at \
              AND julianday(excluded.captured_at) = julianday(documents.captured_at) \
              AND excluded.captured_at = documents.captured_at)"
                .to_string(),
        ]);
    }

    predicates.join(" OR ")
}

fn is_safe_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.to_string_lossy().into_owned();
    s.push_str(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::TempDir;

    fn make_target_db(dir: &Path) -> PathBuf {
        let p = dir.join("refine.db");
        let conn = Connection::open(&p).unwrap();
        crate::infra::prepare_sqlite_db(&conn).unwrap();
        p
    }

    fn make_legacy_db_with_items(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        let conn = Connection::open(&p).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                id TEXT PRIMARY KEY,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .unwrap();
        p
    }

    fn insert_item(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO items \
             (id, item_type, title, summary, content, tags, created_at, updated_at) \
             VALUES (?1, 'knowledge', 'T', 'S', 'C', '[]', \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [id],
        )
        .unwrap();
    }

    fn item_count(conn: &Connection, id: &str) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM items WHERE id=?1", [id], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn no_stale_files_is_noop() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let report = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(report, MigrationReport::NoOp));
    }

    #[test]
    fn migrates_items_from_server_db() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "item-001");
        drop(lc);

        let report = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(
            report,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));
        assert_eq!(
            item_count(&Connection::open(&target).unwrap(), "item-001"),
            1
        );
    }

    #[test]
    fn migrates_server_owned_tables_into_fresh_target() {
        let tmp = TempDir::new().unwrap();
        let target_path = tmp.path().join("refine.db");
        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE conversations (
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
            CREATE TABLE extraction_jobs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                error TEXT
            );
            CREATE TABLE events (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                event_name TEXT NOT NULL,
                source TEXT NOT NULL,
                properties_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        lc.execute(
            "INSERT INTO conversations
             (id, user_id, source, url, title, raw_content, metadata_json, captured_at,
              created_at, status, idempotency_key, item_ids, last_error)
             VALUES
             ('conv-1', 'user-1', 'extension', 'https://example.com/1', 'Title 1', 'Raw 1',
              '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'captured', 'idem-1',
              '[]', NULL)",
            [],
        )
        .unwrap();
        lc.execute(
            "INSERT INTO extraction_jobs
             (id, conversation_id, mode, status, created_at, updated_at, error)
             VALUES
             ('job-1', 'conv-1', 'auto', 'pending', '2026-01-01T00:00:00Z',
              '2026-01-01T00:05:00Z', NULL)",
            [],
        )
        .unwrap();
        lc.execute(
            "INSERT INTO events
             (id, user_id, event_name, source, properties_json, created_at)
             VALUES
             ('event-1', 'user-1', 'conversation_created', 'extension', '{}',
              '2026-01-01T00:10:00Z')",
            [],
        )
        .unwrap();
        drop(lc);

        let report = migrate_stale_dbs(&target_path).unwrap();
        assert!(matches!(
            report,
            MigrationReport::Migrated { rows_copied: 3, .. }
        ));
        assert!(
            legacy.exists(),
            "legacy DB remains discoverable for later reconciliation"
        );

        let tc = Connection::open(&target_path).unwrap();
        let conversation_count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE id='conv-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let job_count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM extraction_jobs WHERE id='job-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let event_count: i64 = tc
            .query_row("SELECT COUNT(*) FROM events WHERE id='event-1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(conversation_count, 1);
        assert_eq!(job_count, 1);
        assert_eq!(event_count, 1);
    }

    #[test]
    fn migrates_subset_of_server_tables_without_manual_target_bootstrap() {
        let tmp = TempDir::new().unwrap();
        let target_path = tmp.path().join("refine.db");
        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE events (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                event_name TEXT NOT NULL,
                source TEXT NOT NULL,
                properties_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );",
        )
        .unwrap();
        lc.execute(
            "INSERT INTO events
             (id, user_id, event_name, source, properties_json, created_at)
             VALUES
             ('event-2', 'user-2', 'conversation_synced', 'extension', '{}',
              '2026-02-02T00:00:00Z')",
            [],
        )
        .unwrap();
        drop(lc);

        let report = migrate_stale_dbs(&target_path).unwrap();
        assert!(matches!(
            report,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));

        let tc = Connection::open(&target_path).unwrap();
        let count: i64 = tc
            .query_row("SELECT COUNT(*) FROM events WHERE id='event-2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn idempotent_second_run() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "item-002");
        drop(lc);

        migrate_stale_dbs(&target).unwrap();
        let report = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(report, MigrationReport::NoOp));
        assert_eq!(
            item_count(&Connection::open(&target).unwrap(), "item-002"),
            1
        );
    }

    #[test]
    fn late_writes_are_reconciled_from_the_discoverable_source() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "first-generation");

        migrate_stale_dbs(&target).unwrap();
        insert_item(&lc, "late-write");
        drop(lc);

        let report = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(
            report,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));
        assert_eq!(
            item_count(&Connection::open(&target).unwrap(), "late-write"),
            1
        );
        assert!(legacy.exists());
    }

    #[test]
    fn later_source_changes_do_not_resurrect_target_deletions() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "deleted-in-target");

        migrate_stale_dbs(&target).unwrap();
        let tc = Connection::open(&target).unwrap();
        tc.execute("DELETE FROM items WHERE id='deleted-in-target'", [])
            .unwrap();
        drop(tc);

        insert_item(&lc, "late-write");
        drop(lc);
        migrate_stale_dbs(&target).unwrap();

        let tc = Connection::open(&target).unwrap();
        assert_eq!(item_count(&tc, "deleted-in-target"), 0);
        assert_eq!(item_count(&tc, "late-write"), 1);
    }

    #[test]
    fn newer_legacy_rows_update_existing_ids_by_version_and_state() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let tc = Connection::open(&target).unwrap();
        insert_item(&tc, "mutable-item");
        drop(tc);

        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "mutable-item");
        lc.execute(
            "UPDATE items SET content='corrected', updated_at='2026-01-02T00:00:00.900Z'
             WHERE id='mutable-item'",
            [],
        )
        .unwrap();
        drop(lc);
        let report = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(
            report,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));
        let corrected: String = Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT content FROM items WHERE id='mutable-item'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(corrected, "corrected");
    }

    #[test]
    fn document_merge_preserves_source_version_when_legacy_lacks_column() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let tc = Connection::open(&target).unwrap();
        tc.execute_batch(
            "INSERT INTO documents
               (id, title, raw_content, source, url, source_version,
                captured_at, created_at, updated_at)
             VALUES
               ('target-doc', 'Old title', 'old body', 'canonical',
                'https://example.com/no-source-version', 'v-target',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
                '2026-01-01T00:00:00Z');",
        )
        .unwrap();
        drop(tc);

        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE documents (
                id TEXT PRIMARY KEY,
                title TEXT,
                raw_content TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO documents
              (id, title, raw_content, source, url, captured_at, created_at, updated_at)
            VALUES
              ('legacy-doc', 'New title', 'new body', 'legacy',
               'https://example.com/no-source-version',
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z',
               '2026-01-02T00:00:00Z');",
        )
        .unwrap();
        drop(lc);

        migrate_stale_dbs(&target).unwrap();

        let document: (String, String, String) = Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT title, raw_content, source_version
                 FROM documents WHERE id='target-doc'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            document,
            ("New title".into(), "new body".into(), "v-target".into())
        );
    }

    #[test]
    fn document_merge_prefers_later_capture_when_updated_at_ties() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let tc = Connection::open(&target).unwrap();
        tc.execute_batch(
            "INSERT INTO documents
               (id, title, raw_content, source, url, source_version,
                captured_at, created_at, updated_at)
             VALUES
               ('target-doc', 'Old title', 'old body', 'canonical',
                'https://example.com/equal-updated', 'v-target',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
                '2026-01-03T00:00:00Z');",
        )
        .unwrap();
        drop(tc);

        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        crate::infra::prepare_sqlite_db(&lc).unwrap();
        lc.execute_batch(
            "INSERT INTO documents
               (id, title, raw_content, source, url, source_version,
                captured_at, created_at, updated_at)
             VALUES
               ('legacy-doc', 'New title', 'new body', 'legacy',
                'https://example.com/equal-updated', 'v-legacy',
                '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z',
                '2026-01-03T00:00:00Z');",
        )
        .unwrap();
        drop(lc);

        migrate_stale_dbs(&target).unwrap();

        let document: (String, String, String, String) = Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT title, raw_content, source_version, captured_at
                 FROM documents WHERE id='target-doc'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            document,
            (
                "New title".into(),
                "new body".into(),
                "v-legacy".into(),
                "2026-01-02T00:00:00Z".into()
            )
        );
    }

    #[test]
    fn document_merge_prefers_later_legacy_rowid_when_timestamps_tie() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE documents (
                id TEXT PRIMARY KEY,
                title TEXT,
                raw_content TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                source_version TEXT,
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            INSERT INTO documents
              (id, title, raw_content, source, url, source_version,
               captured_at, created_at, updated_at)
            VALUES
              ('first-doc', 'First title', 'first body', 'legacy',
               'https://example.com/legacy-duplicate', 'v-first',
               '2026-01-02T00:00:00Z', '2026-01-01T00:00:00Z',
               '2026-01-03T00:00:00Z'),
              ('second-doc', 'Second title', 'second body', 'legacy',
               'https://example.com/legacy-duplicate', 'v-second',
               '2026-01-02T00:00:00Z', '2026-01-01T00:00:00Z',
               '2026-01-03T00:00:00Z');",
        )
        .unwrap();
        drop(lc);

        migrate_stale_dbs(&target).unwrap();

        let document: (String, String, String) = Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT id, raw_content, source_version
                 FROM documents WHERE url='https://example.com/legacy-duplicate'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            document,
            ("first-doc".into(), "second body".into(), "v-second".into())
        );
    }

    #[test]
    fn business_keys_remap_parent_ids_and_internal_tables_are_ignored() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let tc = Connection::open(&target).unwrap();
        tc.execute_batch(
            "INSERT INTO documents
               (id, title, raw_content, source, url, source_version,
                captured_at, created_at, updated_at)
             VALUES
               ('target-doc', 'Old', 'old', 'legacy', 'https://example.com/shared', 'v1',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
             INSERT INTO conversations
               (id, user_id, source, url, raw_content, captured_at, created_at,
                status, idempotency_key, item_ids)
             VALUES
               ('target-conv', 'u', 'legacy', 'https://example.com/conversation', 'old',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'queued',
                'shared-idempotency-key', '[]');",
        )
        .unwrap();
        drop(tc);

        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        crate::infra::prepare_sqlite_db(&lc).unwrap();
        prepare_migration_state(&lc).unwrap();
        lc.execute_batch(
            "INSERT INTO documents
               (id, title, raw_content, source, url, source_version,
                captured_at, created_at, updated_at)
             VALUES
               ('source-doc', '', 'new', 'other-source', 'https://example.com/shared', 'v2',
                '2026-01-02T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z');
             INSERT INTO items
               (id, item_type, title, summary, content, tags, created_at, updated_at, document_id)
             VALUES
               ('mapped-item', 'knowledge', 'T', 'S', 'C', '[]',
                '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', 'source-doc');
             INSERT INTO conversations
               (id, user_id, source, url, raw_content, captured_at, created_at,
                status, idempotency_key, item_ids)
             VALUES
               ('source-conv', 'u', 'legacy', 'https://example.com/conversation', 'new',
                '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'processed',
                'shared-idempotency-key', '[\"mapped-item\"]');
             INSERT INTO extraction_jobs
               (id, conversation_id, mode, status, created_at, updated_at)
             VALUES
               ('mapped-job', 'source-conv', 'auto', 'succeeded',
                '2026-01-01T00:00:00Z', '2026-01-02T00:00:00Z');
             INSERT INTO refine_legacy_migration_state
               (source_path, signature, migrated_at)
             VALUES ('internal', 'must-not-copy', '2026-01-01T00:00:00Z');",
        )
        .unwrap();
        drop(lc);

        migrate_stale_dbs(&target).unwrap();

        let tc = Connection::open(&target).unwrap();
        let document: (String, String, String, String, String) = tc
            .query_row(
                "SELECT id, title, raw_content, source, created_at
                 FROM documents WHERE url='https://example.com/shared'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            document,
            (
                "target-doc".into(),
                "Old".into(),
                "new".into(),
                "legacy".into(),
                "2026-01-01T00:00:00Z".into()
            )
        );
        let item_document: String = tc
            .query_row(
                "SELECT document_id FROM items WHERE id='mapped-item'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_document, "target-doc");
        let conversation: (String, String) = tc
            .query_row(
                "SELECT id, status FROM conversations WHERE idempotency_key='shared-idempotency-key'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(conversation, ("target-conv".into(), "processed".into()));
        let job_conversation: String = tc
            .query_row(
                "SELECT conversation_id FROM extraction_jobs WHERE id='mapped-job'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(job_conversation, "target-conv");
        let internal_count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM refine_legacy_migration_state WHERE source_path='internal'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(internal_count, 0);
    }

    #[test]
    fn signature_detects_middle_change_with_same_size_and_mtime() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("server.db");
        std::fs::write(&source, vec![b'a'; 16 * 1024]).unwrap();
        let fixed = filetime::FileTime::from_unix_time(1_700_000_000, 123_000_000);
        filetime::set_file_mtime(&source, fixed).unwrap();
        let before = source_signature(&source).unwrap();

        let mut bytes = std::fs::read(&source).unwrap();
        bytes[8 * 1024] = b'b';
        std::fs::write(&source, bytes).unwrap();
        filetime::set_file_mtime(&source, fixed).unwrap();
        let after = source_signature(&source).unwrap();

        #[cfg(unix)]
        assert_ne!(before, after);
        #[cfg(not(unix))]
        assert_eq!(before, after);
    }

    #[test]
    fn progressing_backup_is_not_stopped_by_stall_timeout() {
        let tmp = TempDir::new().unwrap();
        let source = Connection::open(tmp.path().join("large.db")).unwrap();
        source
            .execute_batch("CREATE TABLE payload (value BLOB NOT NULL)")
            .unwrap();
        source
            .execute(
                "INSERT INTO payload (value) VALUES (?1)",
                [vec![0x5a; 2 * 1024 * 1024]],
            )
            .unwrap();
        let mut destination = Connection::open(tmp.path().join("backup.db")).unwrap();
        let backup = Backup::new(&source, &mut destination).unwrap();

        run_backup_with_deadline(&backup, Path::new("large.db"), Duration::ZERO).unwrap();
    }

    #[test]
    fn backup_created_before_migration() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        make_legacy_db_with_items(tmp.path(), "server.db");

        migrate_stale_dbs(&target).unwrap();

        assert!(tmp.path().join("server.db.pre-migration.bak").exists());
        assert!(tmp.path().join("server.db").exists());
    }

    #[test]
    fn rollback_on_corrupt_source() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let corrupt = tmp.path().join("server.db");
        std::fs::write(&corrupt, b"not a sqlite database").unwrap();

        let result = migrate_stale_dbs(&target);
        assert!(result.is_err());
        assert!(corrupt.exists(), "corrupt source must be left intact");
        assert!(
            !tmp.path().join("server.db.pre-migration.bak").exists(),
            "an unverifiable raw copy must not masquerade as a valid backup"
        );
        assert!(
            std::fs::read_dir(tmp.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("server.db.pre-migration.forensic-")
                        && entry.path().join("main.db").exists()
                        && entry.path().join("MANIFEST.txt").exists()
                }),
            "corrupt main file must be preserved as an explicitly raw forensic copy"
        );

        let second = migrate_stale_dbs(&target);
        assert!(second.is_err());
        let bundle_count = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("server.db.pre-migration.forensic-")
            })
            .count();
        assert_eq!(bundle_count, 1, "same corrupt generation reuses its bundle");
    }

    #[test]
    fn later_table_failure_rolls_back_all_rows_for_candidate() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE conversations (
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
            CREATE TABLE extraction_jobs (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                error TEXT
            );
            INSERT INTO conversations
              (id, user_id, source, url, raw_content, captured_at, created_at,
               status, idempotency_key, item_ids)
            VALUES
              ('conv-rollback', 'user-1', 'extension', 'https://example.com/rollback',
               'raw', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
               'captured', 'idem-rollback', '[]');
            INSERT INTO extraction_jobs
              (id, conversation_id, mode, status, created_at, updated_at)
            VALUES
              ('job-orphan', 'missing-conversation', 'auto', 'pending',
               '2026-01-01T00:01:00Z', '2026-01-01T00:01:00Z');",
        )
        .unwrap();
        drop(lc);

        let result = migrate_stale_dbs(&target);
        assert!(result.is_err(), "orphan job must fail the candidate import");

        let tc = Connection::open(&target).unwrap();
        let conversation_count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE id='conv-rollback'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let job_count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM extraction_jobs WHERE id='job-orphan'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(conversation_count, 0, "earlier table copy must roll back");
        assert_eq!(job_count, 0);
        assert!(legacy.exists(), "failed source must remain retryable");
        assert!(
            tmp.path().join("server.db.pre-migration.bak").exists(),
            "failure backup must remain available"
        );
    }

    #[test]
    fn live_wal_source_is_backed_up_and_remains_reconcilable() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        insert_item(&lc, "item-from-wal");
        assert!(
            with_suffix(&legacy, "-wal").exists(),
            "fixture must keep committed rows in a live WAL"
        );
        let result = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(
            result,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));
        assert_eq!(
            item_count(&Connection::open(&target).unwrap(), "item-from-wal"),
            1,
            "target import must use the verified WAL-inclusive snapshot"
        );
        assert!(legacy.exists(), "source remains available for later writes");

        let backup = Connection::open(tmp.path().join("server.db.pre-migration.bak")).unwrap();
        assert_eq!(
            item_count(&backup, "item-from-wal"),
            1,
            "SQLite backup must include committed WAL rows"
        );
        drop(backup);
        drop(lc);
    }

    #[test]
    fn retry_keeps_existing_backup_immutable() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let old_backup = tmp.path().join("server.db.pre-migration.bak");
        let backup_conn = Connection::open(&old_backup).unwrap();
        backup_conn
            .execute_batch(
                "CREATE TABLE marker (value TEXT NOT NULL); INSERT INTO marker VALUES ('old');",
            )
            .unwrap();
        drop(backup_conn);
        let original_bytes = std::fs::read(&old_backup).unwrap();
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "retry-item");
        drop(lc);

        migrate_stale_dbs(&target).unwrap();

        assert_eq!(std::fs::read(&old_backup).unwrap(), original_bytes);
        let old_value: String = Connection::open(&old_backup)
            .unwrap()
            .query_row("SELECT value FROM marker", [], |row| row.get(0))
            .unwrap();
        assert_eq!(old_value, "old");
        let leftover_snapshots = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("server.db.pre-migration.bak.tmp-")
            })
            .count();
        assert_eq!(leftover_snapshots, 0);
    }

    #[test]
    fn stale_candidates_only_returns_existing_files() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("refine.db");

        assert!(stale_db_candidates(&target).is_empty());

        std::fs::write(tmp.path().join("server.db"), b"").unwrap();
        let candidates = stale_db_candidates(&target);
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].ends_with("server.db"));
    }
}
