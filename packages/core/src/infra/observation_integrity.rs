use crate::error::{InfraError, InfraResult};
use rusqlite::Connection;

pub(super) fn ensure_triggers(conn: &Connection) -> InfraResult<()> {
    conn.execute_batch(
        r#"
        CREATE TRIGGER IF NOT EXISTS observations_require_document_insert
        BEFORE INSERT ON items
        WHEN NEW.item_type = 'observation' AND NEW.document_id IS NULL
        BEGIN
            SELECT RAISE(ABORT, 'observation requires document_id');
        END;
        CREATE TRIGGER IF NOT EXISTS observations_require_document_update
        BEFORE UPDATE OF item_type, document_id ON items
        WHEN NEW.item_type = 'observation' AND NEW.document_id IS NULL
        BEGIN
            SELECT RAISE(ABORT, 'observation requires document_id');
        END;
        CREATE TABLE IF NOT EXISTS item_link_repair_ledger (
            item_id TEXT PRIMARY KEY,
            target_document_id TEXT NOT NULL,
            evidence_sha256 TEXT NOT NULL,
            rule_version TEXT NOT NULL,
            applied_at TEXT NOT NULL,
            FOREIGN KEY (item_id) REFERENCES items(id) ON DELETE RESTRICT,
            FOREIGN KEY (target_document_id) REFERENCES documents(id) ON DELETE RESTRICT
        );
        CREATE TRIGGER IF NOT EXISTS item_link_repair_ledger_no_update
        BEFORE UPDATE ON item_link_repair_ledger
        BEGIN
            SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
        END;
        CREATE TRIGGER IF NOT EXISTS item_link_repair_ledger_no_delete
        BEFORE DELETE ON item_link_repair_ledger
        BEGIN
            SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
        END;
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

pub(super) fn suspend_for_legacy_import(conn: &Connection) -> InfraResult<()> {
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS observations_require_document_insert;
         DROP TRIGGER IF EXISTS observations_require_document_update;",
    )
    .map_err(|e| InfraError::Database(e.to_string()))
}

pub(super) fn verify_triggers(conn: &Connection) -> InfraResult<()> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name IN (
               'observations_require_document_insert',
               'observations_require_document_update'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if count == 2 {
        Ok(())
    } else {
        Err(InfraError::Database(format!(
            "observation document invariant verification failed: expected 2 triggers, found {count}"
        )))
    }
}
