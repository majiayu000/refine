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
        DROP TRIGGER IF EXISTS observations_require_document_update;
        CREATE TRIGGER observations_require_document_update
        BEFORE UPDATE OF item_type, document_id ON items
        WHEN NEW.item_type = 'observation' AND NEW.document_id IS NULL
          AND NOT (
            OLD.item_type = 'observation' AND OLD.document_id IS NULL
          )
        BEGIN
            SELECT RAISE(ABORT, 'observation requires document_id');
        END;
        CREATE TABLE IF NOT EXISTS item_link_repair_ledger (
            item_id TEXT PRIMARY KEY,
            target_document_id TEXT NOT NULL,
            evidence_sha256 TEXT NOT NULL,
            rule_version TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    migrate_ledger_foreign_keys(conn)?;
    conn.execute_batch(
        r#"
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

fn migrate_ledger_foreign_keys(conn: &Connection) -> InfraResult<()> {
    let foreign_keys: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('item_link_repair_ledger')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if foreign_keys == 0 {
        return Ok(());
    }

    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM item_link_repair_ledger", [], |row| {
            row.get(0)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;
    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS item_link_repair_ledger_no_update;
        DROP TRIGGER IF EXISTS item_link_repair_ledger_no_delete;
        ALTER TABLE item_link_repair_ledger
          RENAME TO item_link_repair_ledger_with_foreign_keys;
        CREATE TABLE item_link_repair_ledger (
            item_id TEXT PRIMARY KEY,
            target_document_id TEXT NOT NULL,
            evidence_sha256 TEXT NOT NULL,
            rule_version TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        INSERT INTO item_link_repair_ledger
          (item_id, target_document_id, evidence_sha256, rule_version, applied_at)
        SELECT item_id, target_document_id, evidence_sha256, rule_version, applied_at
        FROM item_link_repair_ledger_with_foreign_keys;
        DROP TABLE item_link_repair_ledger_with_foreign_keys;
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;
    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM item_link_repair_ledger", [], |row| {
            row.get(0)
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;
    if after != before {
        return Err(InfraError::Database(format!(
            "repair ledger foreign-key migration changed row count: {before}->{after}"
        )));
    }
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

#[cfg(test)]
mod tests {
    use super::ensure_triggers;
    use rusqlite::Connection;

    #[test]
    fn existing_ledger_foreign_keys_are_removed_without_changing_history() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE documents (id TEXT PRIMARY KEY);
             CREATE TABLE items (
               id TEXT PRIMARY KEY, item_type TEXT NOT NULL, document_id TEXT,
               FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE RESTRICT
             );
             INSERT INTO documents VALUES ('document');
             INSERT INTO items VALUES ('item', 'observation', 'document');
             CREATE TABLE item_link_repair_ledger (
               item_id TEXT PRIMARY KEY,
               target_document_id TEXT NOT NULL,
               evidence_sha256 TEXT NOT NULL,
               rule_version TEXT NOT NULL,
               applied_at TEXT NOT NULL,
               FOREIGN KEY (item_id) REFERENCES items(id) ON DELETE RESTRICT,
               FOREIGN KEY (target_document_id) REFERENCES documents(id) ON DELETE RESTRICT
             );
             INSERT INTO item_link_repair_ledger VALUES
               ('item', 'document', 'hash', 'rule', '2026-01-01T00:00:00Z');
             CREATE TRIGGER item_link_repair_ledger_no_update
             BEFORE UPDATE ON item_link_repair_ledger BEGIN
               SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
             END;
             CREATE TRIGGER item_link_repair_ledger_no_delete
             BEFORE DELETE ON item_link_repair_ledger BEGIN
               SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
             END;",
        )
        .unwrap();

        ensure_triggers(&conn).unwrap();

        let history: (String, String, String, String, String) = conn
            .query_row(
                "SELECT item_id, target_document_id, evidence_sha256, rule_version, applied_at
                 FROM item_link_repair_ledger",
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
            history,
            (
                "item".into(),
                "document".into(),
                "hash".into(),
                "rule".into(),
                "2026-01-01T00:00:00Z".into()
            )
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM pragma_foreign_key_list('item_link_repair_ledger')",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute("DELETE FROM items WHERE id = 'item'", [])
            .unwrap();
        conn.execute("DELETE FROM documents WHERE id = 'document'", [])
            .unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM item_link_repair_ledger", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            1
        );
        assert!(conn
            .execute(
                "UPDATE item_link_repair_ledger SET rule_version = 'changed'",
                []
            )
            .unwrap_err()
            .to_string()
            .contains("append-only"));
        assert!(conn
            .execute("DELETE FROM item_link_repair_ledger", [])
            .unwrap_err()
            .to_string()
            .contains("append-only"));
    }
}
