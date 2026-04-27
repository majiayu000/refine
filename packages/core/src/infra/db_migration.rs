use rusqlite::Connection;
use std::path::{Path, PathBuf};

use super::paths::stale_db_candidates;

const SCHEMA_SQL: &str = include_str!("schema.sql");

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
/// On failure the legacy files are left intact and an `Err` is returned —
/// the caller should warn the user but must not abort startup, because the
/// primary database is never written to destructively (only `INSERT OR IGNORE`).
pub fn migrate_stale_dbs(target: &Path) -> Result<MigrationReport, String> {
    let candidates = stale_db_candidates(target);
    if candidates.is_empty() {
        return Ok(MigrationReport::NoOp);
    }

    let conn = Connection::open(target)
        .map_err(|e| format!("failed to open target DB {}: {}", target.display(), e))?;

    init_target_schema(&conn)?;

    let mut sources = Vec::new();
    let mut total_rows = 0usize;

    for candidate in &candidates {
        let bak_path = with_suffix(candidate, ".pre-migration.bak");
        std::fs::copy(candidate, &bak_path)
            .map_err(|e| format!("failed to backup {}: {}", candidate.display(), e))?;

        let attach_sql = format!(
            "ATTACH DATABASE '{}' AS refine_migration_src",
            candidate.to_string_lossy().replace('\'', "''")
        );
        if let Err(e) = conn.execute_batch(&attach_sql) {
            let _ = std::fs::remove_file(&bak_path);
            return Err(format!("failed to attach {}: {}", candidate.display(), e));
        }

        let result = copy_all_tables(&conn, "refine_migration_src");
        let _ = conn.execute_batch("DETACH DATABASE refine_migration_src");

        let rows = result.map_err(|e| {
            let _ = std::fs::remove_file(&bak_path);
            format!("migration of {} failed: {}", candidate.display(), e)
        })?;

        let migrated_path = with_suffix(candidate, ".migrated");
        std::fs::rename(candidate, &migrated_path)
            .map_err(|e| format!("failed to rename {}: {}", candidate.display(), e))?;

        // Move companion WAL/SHM files so they stay associated with the renamed DB.
        for ext in ["-wal", "-shm"] {
            let companion = with_suffix(candidate, ext);
            if companion.exists() {
                let _ = std::fs::rename(&companion, with_suffix(&migrated_path, ext));
            }
        }

        sources.push(candidate.clone());
        total_rows += rows;
    }

    Ok(MigrationReport::Migrated {
        sources,
        rows_copied: total_rows,
    })
}

fn init_target_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA_SQL)
        .map_err(|e| format!("failed to initialise target schema: {}", e))
}

fn copy_all_tables(conn: &Connection, legacy_alias: &str) -> Result<usize, String> {
    let target_tables = list_base_tables(conn, "main")?;
    let legacy_tables = list_base_tables(conn, legacy_alias)?;

    let mut total = 0usize;
    for table in &legacy_tables {
        if !target_tables.contains(table) {
            continue;
        }
        total += copy_table(conn, table, legacy_alias)?;
    }
    Ok(total)
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
        init_target_schema(&conn).unwrap();
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
    fn migrates_matching_tables_from_server_db() {
        let tmp = TempDir::new().unwrap();
        let target_path = tmp.path().join("refine.db");

        // Target: base schema + a conversations table
        let tc = Connection::open(&target_path).unwrap();
        init_target_schema(&tc).unwrap();
        tc.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (id TEXT PRIMARY KEY, data TEXT)",
        )
        .unwrap();
        drop(tc);

        // Legacy: conversations only
        let legacy = tmp.path().join("server.db");
        let lc = Connection::open(&legacy).unwrap();
        lc.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversations (id TEXT PRIMARY KEY, data TEXT)",
        )
        .unwrap();
        lc.execute("INSERT INTO conversations VALUES ('conv-1', 'hello')", [])
            .unwrap();
        drop(lc);

        migrate_stale_dbs(&target_path).unwrap();

        let tc = Connection::open(&target_path).unwrap();
        let count: i64 = tc
            .query_row(
                "SELECT COUNT(*) FROM conversations WHERE id='conv-1'",
                [],
                |r| r.get(0),
            )
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
