use rusqlite::{backup::Backup, backup::StepResult, params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
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
/// deletions are not propagated because legacy schemas have no tombstones. On failure,
/// all writes for the failing source are rolled back and an `Err` is returned.
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

    let mut sources = Vec::new();
    let mut total_rows = 0usize;

    for candidate in &candidates {
        let signature_before = source_signature(candidate)?;
        if !force_reconcile()
            && migration_signature(&conn, candidate)?.as_deref() == Some(&signature_before)
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

        // Import the exact verified snapshot. Concurrent rows committed after
        // the snapshot remain in the legacy source for the next reconciliation.
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
            let rows = copy_all_tables(&tx, "refine_migration_src")?;
            let signature_after = source_signature(candidate)?;
            if signature_after == signature_before {
                save_migration_signature(&tx, candidate, &signature_after)?;
            }
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
            migrated_at TEXT NOT NULL
        )",
    )
    .map_err(|e| format!("failed to prepare legacy migration state: {e}"))
}

fn migration_signature(conn: &Connection, source: &Path) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT signature FROM refine_legacy_migration_state WHERE source_path=?1",
        [source.to_string_lossy().as_ref()],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("failed to read legacy migration state: {e}"))
}

fn save_migration_signature(
    conn: &Connection,
    source: &Path,
    signature: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO refine_legacy_migration_state (source_path, signature, migrated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(source_path) DO UPDATE SET
           signature=excluded.signature, migrated_at=excluded.migrated_at",
        params![
            source.to_string_lossy().as_ref(),
            signature,
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
                let mut file = std::fs::File::open(&path)
                    .map_err(|e| format!("failed to open {} for signature: {e}", path.display()))?;
                let mut hasher = Sha256::new();
                let mut sample = vec![0u8; 4096.min(metadata.len() as usize)];
                if !sample.is_empty() {
                    file.read_exact(&mut sample).map_err(|e| {
                        format!("failed to read signature sample {}: {e}", path.display())
                    })?;
                    hasher.update(&sample);
                    if metadata.len() > sample.len() as u64 {
                        file.seek(SeekFrom::End(-(sample.len() as i64)))
                            .map_err(|e| {
                                format!("failed to seek signature sample {}: {e}", path.display())
                            })?;
                        file.read_exact(&mut sample).map_err(|e| {
                            format!("failed to read tail signature {}: {e}", path.display())
                        })?;
                        hasher.update(&sample);
                    }
                }
                parts.push(format!(
                    "{suffix}:{}:{}:{:x}",
                    metadata.len(),
                    modified.as_nanos(),
                    hasher.finalize()
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
            run_backup_with_deadline(&backup, source, Duration::from_secs(5))?;
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

        // Keep the first recovery point immutable. Later reconciliation runs
        // import from a verified temporary snapshot and remove it on drop.
        if destination.exists() {
            Ok(MigrationSnapshot {
                path: temporary.clone(),
                remove_on_drop: true,
            })
        } else {
            std::fs::rename(&temporary, destination).map_err(|e| {
                format!(
                    "failed to publish backup {} for {}: {}",
                    destination.display(),
                    source.display(),
                    e
                )
            })?;
            Ok(MigrationSnapshot {
                path: destination.to_path_buf(),
                remove_on_drop: false,
            })
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn run_backup_with_deadline(
    backup: &Backup<'_, '_>,
    source: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let step = backup
            .step(128)
            .map_err(|e| format!("failed to backup {}: {}", source.display(), e))?;
        match step {
            StepResult::Done => return Ok(()),
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => {
                if started.elapsed() >= timeout {
                    return Err(format!(
                        "timed out after {}ms backing up {}",
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

fn forensic_bundle_path(source: &Path, unique: uuid::Uuid) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "legacy.db".to_string());
    parent.join(format!("{name}.pre-migration.forensic-{unique}"))
}

fn preserve_forensic_bundle(source: &Path) -> Result<(), String> {
    let bundle = forensic_bundle_path(source, uuid::Uuid::new_v4());
    std::fs::create_dir(&bundle).map_err(|e| {
        format!(
            "failed to create forensic bundle {}: {}",
            bundle.display(),
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
        let forensic = bundle.join(&name);
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
        bundle.join("MANIFEST.txt"),
        format!(
            "source={}\ncomplete=false\nfiles={}\n",
            source.display(),
            copied.join(",")
        ),
    )
    .map_err(|e| {
        format!(
            "failed to write forensic manifest {}: {}",
            bundle.display(),
            e
        )
    })?;
    Ok(())
}

fn copy_all_tables(conn: &Connection, legacy_alias: &str) -> Result<usize, String> {
    let target_tables = list_base_tables(conn, "main")?;
    let legacy_tables = list_base_tables(conn, legacy_alias)?;
    let ordered_tables = migration_copy_order(&legacy_tables);

    let mut total = 0usize;
    for table in &ordered_tables {
        if !target_tables.contains(table) {
            continue;
        }
        total += copy_table(conn, table, legacy_alias)?;
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
    let mut ordered = Vec::with_capacity(legacy_tables.len());
    for table in preferred {
        if legacy_tables.iter().any(|existing| existing == table) {
            ordered.push(table.to_string());
        }
    }
    for table in legacy_tables {
        if !ordered.iter().any(|existing| existing == table) {
            ordered.push(table.clone());
        }
    }
    ordered
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

fn copy_table(conn: &Connection, table: &str, legacy_alias: &str) -> Result<usize, String> {
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
        "items" | "documents" | "extraction_jobs" if common.iter().any(|c| c == "updated_at") => {
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
    let sql = format!(
        "INSERT INTO {table} ({col_list}) \
         SELECT {col_list} FROM {legacy_alias}.{table} WHERE true {conflict}"
    );
    conn.execute(&sql, [])
        .map_err(|e| format!("failed to copy table {table}: {e}"))
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
            "legacy DB remains for later reconciliation"
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
    fn later_updates_to_existing_ids_reconcile_by_version_and_state() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        let legacy = make_legacy_db_with_items(tmp.path(), "server.db");
        let lc = Connection::open(&legacy).unwrap();
        insert_item(&lc, "mutable-item");
        drop(lc);
        migrate_stale_dbs(&target).unwrap();

        let lc = Connection::open(&legacy).unwrap();
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

        let lc = Connection::open(&legacy).unwrap();
        lc.execute(
            "UPDATE items SET content='stale', updated_at='2025-12-31T23:59:59Z'
             WHERE id='mutable-item'",
            [],
        )
        .unwrap();
        drop(lc);
        migrate_stale_dbs(&target).unwrap();
        let still_corrected: String = Connection::open(&target)
            .unwrap()
            .query_row(
                "SELECT content FROM items WHERE id='mutable-item'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still_corrected, "corrected");
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
    fn live_wal_source_is_backed_up_and_late_writes_reconcile_next_run() {
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
        assert!(
            legacy.exists(),
            "source stays discoverable for late writers"
        );

        let backup = Connection::open(tmp.path().join("server.db.pre-migration.bak")).unwrap();
        assert_eq!(
            item_count(&backup, "item-from-wal"),
            1,
            "SQLite backup must include committed WAL rows"
        );
        drop(backup);

        insert_item(&lc, "late-item");
        let second = migrate_stale_dbs(&target).unwrap();
        assert!(matches!(
            second,
            MigrationReport::Migrated { rows_copied: 1, .. }
        ));
        assert_eq!(
            item_count(&Connection::open(&target).unwrap(), "late-item"),
            1,
            "a write from an already-open legacy connection must reconcile later"
        );
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
