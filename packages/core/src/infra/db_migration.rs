use rusqlite::{backup::Backup, Connection};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

/// Copies rows from any legacy databases into `target` and renames each legacy
/// file to `<name>.migrated` so the migration only runs once.
///
/// On success the legacy files no longer exist at their original paths.
/// On failure the legacy files and their pre-migration backups are left intact,
/// all writes for the failing source are rolled back, and an `Err` is returned.
/// The caller should warn the user but must not abort startup.
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
    drop(conn);

    let mut sources = Vec::new();
    let mut total_rows = 0usize;

    for candidate in &candidates {
        let bak_path = with_suffix(candidate, ".pre-migration.bak");
        let migrated_path = with_suffix(candidate, ".migrated");
        ensure_archive_destinations_free(candidate, &migrated_path)?;
        let (source_conn, snapshot_path) = match create_consistent_backup(candidate, &bak_path) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                preserve_forensic_backup(candidate, &bak_path)?;
                return Err(error);
            }
        };

        // Import the immutable, verified snapshot while the source write lock
        // remains held. A legacy writer cannot race a new WAL commit between
        // backup and archival.
        let conn = Connection::open(target)
            .map_err(|e| format!("failed to reopen target DB {}: {}", target.display(), e))?;
        crate::infra::configure_sqlite_connection(&conn)
            .map_err(|e| format!("failed to configure target connection: {}", e))?;

        let attach_sql = format!(
            "ATTACH DATABASE '{}' AS refine_migration_src",
            snapshot_path.to_string_lossy().replace('\'', "''")
        );
        if let Err(e) = conn.execute_batch(&attach_sql) {
            return Err(format!("failed to attach {}: {}", candidate.display(), e));
        }

        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("failed to start migration transaction: {e}"))?;
        let rows = copy_all_tables(&tx, "refine_migration_src")
            .map_err(|e| format!("migration of {} failed: {}", candidate.display(), e))?;

        // Archive while the source write lock and target transaction are both
        // active. A target commit failure restores the source filenames before
        // reporting failure, so an error never leaves only one side committed.
        let archived = archive_locked_source(candidate, &migrated_path)?;
        if let Err(error) = tx.commit() {
            let restore_note = restore_archived_source(&archived);
            return Err(format!(
                "failed to commit migration transaction: {error}{restore_note}"
            ));
        }
        drop(conn);
        drop(source_conn);

        sources.push(candidate.clone());
        total_rows += rows;
    }

    Ok(MigrationReport::Migrated {
        sources,
        rows_copied: total_rows,
    })
}

fn create_consistent_backup(
    source: &Path,
    destination: &Path,
) -> Result<(Connection, PathBuf), String> {
    let source_conn = Connection::open(source)
        .map_err(|e| format!("failed to open legacy DB {}: {}", source.display(), e))?;
    source_conn
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| format!("failed to configure legacy DB {}: {}", source.display(), e))?;
    let version_before: i64 = source_conn
        .pragma_query_value(None, "data_version", |row| row.get(0))
        .map_err(|e| format!("failed to inspect legacy DB {}: {}", source.display(), e))?;
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
            backup
                .run_to_completion(128, Duration::from_millis(10), None)
                .map_err(|e| format!("failed to backup {}: {}", source.display(), e))?;
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

        // Keep an earlier recovery point immutable. A retry publishes a
        // versioned snapshot and imports from that exact verified file.
        let published = if destination.exists() {
            with_suffix(destination, &format!(".{unique}"))
        } else {
            destination.to_path_buf()
        };
        std::fs::rename(&temporary, &published).map_err(|e| {
            format!(
                "failed to publish backup {} for {}: {}",
                published.display(),
                source.display(),
                e
            )
        })?;
        source_conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("failed to lock legacy DB {}: {}", source.display(), e))?;
        let version_after: i64 = source_conn
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .map_err(|e| format!("failed to recheck legacy DB {}: {}", source.display(), e))?;
        if version_after != version_before {
            return Err(format!(
                "legacy DB {} changed while its migration snapshot was created; retry migration",
                source.display()
            ));
        }
        Ok((source_conn, published))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
        preserve_forensic_backup(source, destination)?;
    }
    result
}

fn preserve_forensic_backup(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Ok(());
    }
    let temporary = with_suffix(destination, &format!(".raw-tmp-{}", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temporary).map_err(|e| {
        format!(
            "failed to preserve forensic backup of {} as {}: {}",
            source.display(),
            destination.display(),
            e
        )
    })?;
    if let Err(error) = std::fs::rename(&temporary, destination) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "failed to publish forensic backup {}: {}",
            destination.display(),
            error
        ));
    }
    Ok(())
}

fn ensure_archive_destinations_free(source: &Path, destination: &Path) -> Result<(), String> {
    for (from, to) in archive_paths(source, destination) {
        if from.exists() && to.exists() {
            return Err(format!(
                "cannot archive {} because destination {} already exists",
                source.display(),
                to.display()
            ));
        }
    }
    Ok(())
}

fn archive_locked_source(
    source: &Path,
    destination: &Path,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let paths: Vec<(PathBuf, PathBuf)> = archive_paths(source, destination)
        .into_iter()
        .filter(|(from, _)| from.exists())
        .collect();
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in paths {
        if let Err(error) = std::fs::rename(&from, &to) {
            let mut rollback_errors = Vec::new();
            for (original, archived) in renamed.iter().rev() {
                if let Err(rollback_error) = std::fs::rename(archived, original) {
                    rollback_errors.push(rollback_error.to_string());
                }
            }
            let rollback_note = if rollback_errors.is_empty() {
                String::new()
            } else {
                format!(
                    "; archive rollback also failed: {}",
                    rollback_errors.join(", ")
                )
            };
            return Err(format!(
                "failed to archive {} as {}: {}{}",
                from.display(),
                to.display(),
                error,
                rollback_note
            ));
        }
        renamed.push((from, to));
    }
    Ok(renamed)
}

fn restore_archived_source(archived: &[(PathBuf, PathBuf)]) -> String {
    let mut errors = Vec::new();
    for (original, destination) in archived.iter().rev() {
        if let Err(error) = std::fs::rename(destination, original) {
            errors.push(format!(
                "{} -> {}: {}",
                destination.display(),
                original.display(),
                error
            ));
        }
    }
    if errors.is_empty() {
        String::new()
    } else {
        format!("; source restore also failed: {}", errors.join(", "))
    }
}

fn archive_paths(source: &Path, destination: &Path) -> Vec<(PathBuf, PathBuf)> {
    ["-wal", "-shm", ""]
        .into_iter()
        .map(|suffix| {
            if suffix.is_empty() {
                (source.to_path_buf(), destination.to_path_buf())
            } else {
                (
                    with_suffix(source, suffix),
                    with_suffix(destination, suffix),
                )
            }
        })
        .collect()
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
    let sql = format!(
        "INSERT OR IGNORE INTO {table} ({col_list}) \
         SELECT {col_list} FROM {legacy_alias}.{table}"
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
            !legacy.exists(),
            "legacy DB should be renamed after success"
        );
        assert!(tmp.path().join("server.db.migrated").exists());

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
    fn backup_created_before_migration() {
        let tmp = TempDir::new().unwrap();
        let target = make_target_db(tmp.path());
        make_legacy_db_with_items(tmp.path(), "server.db");

        migrate_stale_dbs(&target).unwrap();

        assert!(tmp.path().join("server.db.pre-migration.bak").exists());
        assert!(tmp.path().join("server.db.migrated").exists());
        assert!(!tmp.path().join("server.db").exists());
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
            tmp.path().join("server.db.pre-migration.bak").exists(),
            "failure backup must be preserved"
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
        assert!(!tmp.path().join("server.db.migrated").exists());
    }

    #[test]
    fn live_wal_source_is_backed_up_and_archived_with_companions() {
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
        let had_wal = with_suffix(&legacy, "-wal").exists();
        let had_shm = with_suffix(&legacy, "-shm").exists();

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
        assert!(!legacy.exists());
        assert!(tmp.path().join("server.db.migrated").exists());
        assert!(!with_suffix(&legacy, "-wal").exists());
        assert!(!with_suffix(&legacy, "-shm").exists());
        assert_eq!(
            tmp.path().join("server.db.migrated-wal").exists(),
            had_wal,
            "WAL must move with the archived source when present"
        );
        assert_eq!(
            tmp.path().join("server.db.migrated-shm").exists(),
            had_shm,
            "SHM must move with the archived source when present"
        );

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
        let versioned_backups = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("server.db.pre-migration.bak.")
            })
            .count();
        assert_eq!(versioned_backups, 1);
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
