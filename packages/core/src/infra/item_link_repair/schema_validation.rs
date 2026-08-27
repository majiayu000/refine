use super::db_error;
use crate::error::{InfraError, InfraResult};
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub(super) fn validate_current_tables(conn: &Connection) -> InfraResult<()> {
    require_columns(
        conn,
        "items",
        &["id", "item_type", "created_at", "updated_at", "document_id"],
    )?;
    require_columns(conn, "documents", &["id", "title", "created_at"])
}

pub(super) fn validate_evidence_tables(conn: &Connection) -> InfraResult<()> {
    require_columns(
        conn,
        "items",
        &["id", "item_type", "title", "created_at", "document_id"],
    )?;
    require_columns(conn, "documents", &["id"])
}

pub(super) fn validate_apply_schema(conn: &Connection) -> InfraResult<()> {
    validate_current_tables(conn)?;
    require_columns(
        conn,
        "item_link_repair_ledger",
        &[
            "item_id",
            "target_document_id",
            "evidence_sha256",
            "rule_version",
            "applied_at",
        ],
    )?;
    require_primary_key(conn)?;
    require_link_constraints(conn)?;
    let insert_trigger_sql = require_trigger_structure(conn)?;
    require_guard_behavior(conn, &insert_trigger_sql)
}

fn require_primary_key(conn: &Connection) -> InfraResult<()> {
    let primary_key_columns: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('item_link_repair_ledger')
             WHERE name = 'item_id' AND pk = 1",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if primary_key_columns == 1 {
        Ok(())
    } else {
        Err(schema_error("repair ledger item_id is not the primary key"))
    }
}

fn require_link_constraints(conn: &Connection) -> InfraResult<()> {
    let ledger_foreign_keys: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('item_link_repair_ledger')",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let item_document_fk = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_list('items')
             WHERE \"from\" = 'document_id' AND \"table\" = 'documents' AND \"to\" = 'id'
               AND UPPER(on_delete) IN ('RESTRICT', 'NO ACTION')",
            [],
            |row| row.get::<_, u64>(0),
        )
        .map_err(db_error)?;
    if ledger_foreign_keys == 0 && item_document_fk == 1 {
        Ok(())
    } else {
        Err(schema_error(
            "repair ledger must not reference mutable business rows, and item links require a fail-closed document foreign key",
        ))
    }
}

fn require_trigger_structure(conn: &Connection) -> InfraResult<String> {
    let mut statement = conn
        .prepare(
            "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND name IN
             ('observations_require_document_insert',
              'observations_require_document_update',
              'item_link_repair_ledger_no_update',
              'item_link_repair_ledger_no_delete')",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut triggers = HashMap::new();
    for row in rows {
        let (name, sql) = row.map_err(db_error)?;
        triggers.insert(name, sql);
    }
    let requirements: [(&str, &[&str]); 4] = [
        (
            "observations_require_document_insert",
            &[
                "before insert on items",
                "new.item_type = 'observation'",
                "new.document_id is null",
                "raise(abort, 'observation requires document_id')",
            ],
        ),
        (
            "observations_require_document_update",
            &[
                "before update of item_type, document_id on items",
                "new.item_type = 'observation'",
                "new.document_id is null",
                "and not ( old.item_type = 'observation' and old.document_id is null )",
                "raise(abort, 'observation requires document_id')",
            ],
        ),
        (
            "item_link_repair_ledger_no_update",
            &[
                "before update on item_link_repair_ledger",
                "item_link_repair_ledger is append-only",
                "raise(abort",
                "begin",
            ],
        ),
        (
            "item_link_repair_ledger_no_delete",
            &[
                "before delete on item_link_repair_ledger",
                "item_link_repair_ledger is append-only",
                "raise(abort",
                "begin",
            ],
        ),
    ];
    for (name, fragments) in requirements {
        let Some(raw_sql) = triggers.get(name) else {
            return Err(schema_error(&format!("missing required trigger {name}")));
        };
        let sql = normalize_sql(raw_sql);
        if fragments.iter().any(|fragment| !sql.contains(fragment)) {
            return Err(schema_error(&format!(
                "required trigger {name} has an unexpected body"
            )));
        }
    }
    triggers
        .remove("observations_require_document_insert")
        .ok_or_else(|| schema_error("missing required Observation insert trigger"))
}

#[derive(Debug, PartialEq, Eq)]
struct ProbeSnapshot {
    items: i64,
    documents: i64,
    ledger: i64,
    fts: i64,
}

fn require_guard_behavior(conn: &Connection, insert_trigger_sql: &str) -> InfraResult<()> {
    let before = probe_snapshot(conn)?;
    conn.execute_batch("SAVEPOINT refine_apply_schema_guard_probe")
        .map_err(db_error)?;

    let probe_result = run_guard_probes(conn, insert_trigger_sql);
    let rollback_result = conn
        .execute_batch(
            "ROLLBACK TO refine_apply_schema_guard_probe;
             RELEASE refine_apply_schema_guard_probe;",
        )
        .map_err(db_error);
    rollback_result?;

    let after = probe_snapshot(conn)?;
    if after != before {
        return Err(schema_error(&format!(
            "guard behavior probe changed persistent state: before={before:?}, after={after:?}"
        )));
    }
    probe_result
}

fn run_guard_probes(conn: &Connection, insert_trigger_sql: &str) -> InfraResult<()> {
    let nonce = Uuid::new_v4().simple().to_string();
    let document_id = format!("__refine_guard_probe_document_{nonce}");
    let linked_id = format!("__refine_guard_probe_linked_{nonce}");
    let detached_id = format!("__refine_guard_probe_detached_{nonce}");
    let new_id = format!("__refine_guard_probe_new_{nonce}");

    conn.execute(
        "INSERT INTO documents
           (id, title, raw_content, source, url, captured_at, created_at, updated_at)
         VALUES (?1, 'guard probe', '', 'repair-schema-probe', ?2,
                 '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z',
                 '2000-01-01T00:00:00Z')",
        params![document_id, format!("refine-schema-probe://{nonce}")],
    )
    .map_err(|error| schema_error(&format!("guard probe could not insert document: {error}")))?;
    conn.execute(
        "INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES (?1, 'observation', 'linked before', '', '', '[]', NULL,
                 '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z', ?2, NULL)",
        params![linked_id, document_id],
    )
    .map_err(|error| {
        schema_error(&format!(
            "guard probe rejected a valid linked Observation: {error}"
        ))
    })?;

    conn.execute_batch("DROP TRIGGER observations_require_document_insert")
        .map_err(db_error)?;
    conn.execute(
        "INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES (?1, 'observation', 'detached before', '', '', '[]', NULL,
                 '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z', NULL, NULL)",
        [&detached_id],
    )
    .map_err(db_error)?;
    conn.execute_batch(insert_trigger_sql).map_err(|error| {
        schema_error(&format!(
            "guard probe could not restore Observation insert trigger: {error}"
        ))
    })?;

    let detached_updated = conn
        .execute(
            "UPDATE items
             SET item_type = 'observation', document_id = NULL,
                 title = 'detached after', content = 'detached after'
             WHERE id = ?1",
            [&detached_id],
        )
        .map_err(|error| {
            schema_error(&format!(
                "guard rejected a non-link edit of a historical detached Observation: {error}"
            ))
        })?;
    if detached_updated != 1
        || conn
            .query_row(
                "SELECT COUNT(*) FROM items
                 WHERE id = ?1 AND title = 'detached after' AND document_id IS NULL",
                [&detached_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(db_error)?
            != 1
    {
        return Err(schema_error(
            "guard did not preserve a historical detached Observation edit",
        ));
    }

    if conn
        .execute(
            "UPDATE items SET document_id = NULL WHERE id = ?1",
            [&linked_id],
        )
        .is_ok()
    {
        return Err(schema_error(
            "Observation update guard allowed a linked row to become detached",
        ));
    }
    let linked_document: Option<String> = conn
        .query_row(
            "SELECT document_id FROM items WHERE id = ?1",
            [&linked_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if linked_document.as_deref() != Some(document_id.as_str()) {
        return Err(schema_error(
            "rejected Observation detach changed the linked row",
        ));
    }

    if conn
        .execute(
            "INSERT INTO items
               (id, item_type, title, summary, content, tags, source,
                created_at, updated_at, document_id, excerpt)
             VALUES (?1, 'observation', 'new detached', '', '', '[]', NULL,
                     '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z', NULL, NULL)",
            [&new_id],
        )
        .is_ok()
    {
        return Err(schema_error(
            "Observation insert guard allowed a new detached row",
        ));
    }
    if conn
        .query_row(
            "SELECT COUNT(*) FROM items WHERE id = ?1",
            [&new_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(db_error)?
        != 0
    {
        return Err(schema_error(
            "rejected detached Observation insert changed the items table",
        ));
    }

    conn.execute(
        "INSERT INTO item_link_repair_ledger
           (item_id, target_document_id, evidence_sha256, rule_version, applied_at)
         VALUES (?1, ?2, 'probe-hash', 'probe-rule', '2000-01-01T00:00:00Z')",
        params![detached_id, document_id],
    )
    .map_err(|error| schema_error(&format!("guard probe could not append ledger row: {error}")))?;
    if conn
        .execute(
            "UPDATE item_link_repair_ledger
             SET rule_version = 'tampered' WHERE item_id = ?1",
            [&detached_id],
        )
        .is_ok()
    {
        return Err(schema_error("repair ledger update guard is ineffective"));
    }
    let ledger_rule: String = conn
        .query_row(
            "SELECT rule_version FROM item_link_repair_ledger WHERE item_id = ?1",
            [&detached_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if ledger_rule != "probe-rule" {
        return Err(schema_error("rejected ledger update changed history"));
    }
    if conn
        .execute(
            "DELETE FROM item_link_repair_ledger WHERE item_id = ?1",
            [&detached_id],
        )
        .is_ok()
    {
        return Err(schema_error("repair ledger delete guard is ineffective"));
    }
    if conn
        .query_row(
            "SELECT COUNT(*) FROM item_link_repair_ledger WHERE item_id = ?1",
            [&detached_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(db_error)?
        != 1
    {
        return Err(schema_error("rejected ledger delete changed history"));
    }
    Ok(())
}

fn probe_snapshot(conn: &Connection) -> InfraResult<ProbeSnapshot> {
    Ok(ProbeSnapshot {
        items: scalar(conn, "SELECT COUNT(*) FROM items")?,
        documents: scalar(conn, "SELECT COUNT(*) FROM documents")?,
        ledger: scalar(conn, "SELECT COUNT(*) FROM item_link_repair_ledger")?,
        fts: scalar(conn, "SELECT COUNT(*) FROM items_fts")?,
    })
}

fn scalar(conn: &Connection, sql: &str) -> InfraResult<i64> {
    conn.query_row(sql, [], |row| row.get(0)).map_err(db_error)
}

fn require_columns(conn: &Connection, table: &str, required: &[&str]) -> InfraResult<()> {
    let sql = match table {
        "items" => "PRAGMA table_info(items)",
        "documents" => "PRAGMA table_info(documents)",
        "item_link_repair_ledger" => "PRAGMA table_info(item_link_repair_ledger)",
        _ => return Err(InfraError::Database(format!("unsupported table {table}"))),
    };
    let mut stmt = conn.prepare(sql).map_err(db_error)?;
    let names: HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error)?;
    let missing: Vec<_> = required
        .iter()
        .filter(|column| !names.contains(**column))
        .copied()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(InfraError::Database(format!(
            "{table} schema mismatch; missing columns: {}",
            missing.join(", ")
        )))
    }
}

fn normalize_sql(sql: &str) -> String {
    sql.to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn schema_error(detail: &str) -> InfraError {
    InfraError::Database(format!(
        "current DB is missing the fail-closed link schema: {detail}; install/open the merged runtime before apply"
    ))
}
