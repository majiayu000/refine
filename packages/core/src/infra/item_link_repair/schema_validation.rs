use super::db_error;
use crate::error::{InfraError, InfraResult};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

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
    require_trigger_bodies(conn)
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

fn require_trigger_bodies(conn: &Connection) -> InfraResult<()> {
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
        triggers.insert(name, normalize_sql(&sql));
    }
    let requirements = [
        (
            "observations_require_document_insert",
            [
                "before insert on items",
                "new.item_type = 'observation'",
                "new.document_id is null",
                "raise(abort, 'observation requires document_id')",
            ],
        ),
        (
            "observations_require_document_update",
            [
                "before update of item_type, document_id on items",
                "new.item_type = 'observation'",
                "new.document_id is null",
                "raise(abort, 'observation requires document_id')",
            ],
        ),
        (
            "item_link_repair_ledger_no_update",
            [
                "before update on item_link_repair_ledger",
                "item_link_repair_ledger is append-only",
                "raise(abort",
                "begin",
            ],
        ),
        (
            "item_link_repair_ledger_no_delete",
            [
                "before delete on item_link_repair_ledger",
                "item_link_repair_ledger is append-only",
                "raise(abort",
                "begin",
            ],
        ),
    ];
    for (name, fragments) in requirements {
        let Some(sql) = triggers.get(name) else {
            return Err(schema_error(&format!("missing required trigger {name}")));
        };
        if fragments.iter().any(|fragment| !sql.contains(fragment)) {
            return Err(schema_error(&format!(
                "required trigger {name} has an unexpected body"
            )));
        }
    }
    Ok(())
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
