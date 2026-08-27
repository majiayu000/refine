use super::*;
use rusqlite::{Connection, TransactionBehavior};
use std::time::Duration;

#[test]
fn invalid_second_group_still_blocks_the_shared_target() {
    let mut current_items = detached_items(&["item-1", "item-2"]);
    current_items.get_mut("item-2").unwrap().document_id = Some("already-linked".into());
    let plan = build_plan_from_rows(
        vec![
            evidence_item("shadow-1", "item-1", "same summary", 1_000),
            evidence_item("shadow-2", "item-2", "same summary", 1_000),
        ],
        vec![current_document("target", "same summary", 1_000)],
        current_items,
        "hash".into(),
    );

    assert_eq!(plan.stats.target_conflicts, 2);
    assert_eq!(plan.stats.candidate_groups, 0);
    assert_eq!(plan.stats.candidate_items, 0);
}

#[test]
fn multiple_matches_in_one_group_block_an_exact_group_claiming_the_same_target() {
    let plan = build_plan_from_rows(
        vec![
            evidence_item("exact-group", "exact-item", "shared", 1_000),
            evidence_item("multiple-group", "multiple-1", "shared", 1_000),
            evidence_item("multiple-group", "multiple-2", "shared", 1_000),
        ],
        vec![current_document("shared-target", "shared", 1_000)],
        detached_items(&["exact-item", "multiple-1", "multiple-2"]),
        "hash".into(),
    );

    assert_eq!(plan.stats.ambiguous_groups, 1);
    assert_eq!(plan.stats.target_conflicts, 2);
    assert_eq!(plan.stats.candidate_groups, 0);
    assert_eq!(plan.stats.candidate_items, 0);
}

#[test]
fn multiple_matches_for_a_distinct_target_do_not_block_an_exact_group() {
    let plan = build_plan_from_rows(
        vec![
            evidence_item("exact-group", "exact-item", "exact", 1_000),
            evidence_item("multiple-group", "multiple-1", "multiple", 2_000),
            evidence_item("multiple-group", "multiple-2", "multiple", 2_000),
        ],
        vec![
            current_document("exact-target", "exact", 1_000),
            current_document("multiple-target", "multiple", 2_000),
        ],
        detached_items(&["exact-item", "multiple-1", "multiple-2"]),
        "hash".into(),
    );

    assert_eq!(plan.stats.ambiguous_groups, 1);
    assert_eq!(plan.stats.target_conflicts, 0);
    assert_eq!(plan.stats.candidate_groups, 1);
    assert_eq!(plan.stats.candidate_items, 1);
    assert_eq!(plan.candidates[0].target_document_id, "exact-target");
}

#[test]
fn audit_orders_rfc3339_offsets_by_actual_instant() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("current.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "DROP TRIGGER observations_require_document_insert;
         UPDATE items
         SET created_at = '2026-01-01T01:00:00+02:00',
             updated_at = '2026-01-01T01:00:00+02:00'
         WHERE id = 'item';
         INSERT INTO items
           (id, item_type, title, summary, content, tags, source,
            created_at, updated_at, document_id, excerpt)
         VALUES
           ('newer-instant', 'observation', 'newer', 'newer', '', '[]', NULL,
            '2026-01-01T00:30:00Z', '2026-01-01T00:45:00Z', NULL, NULL);",
    )
    .unwrap();
    drop(conn);

    let audit = audit_detached_observations(&current).unwrap();
    assert_eq!(audit.detached_observations, 2);
    assert_eq!(
        audit.newest_detached_created_at.as_deref(),
        Some("2026-01-01T00:30:00+00:00")
    );
    assert_eq!(
        audit.newest_detached_updated_at.as_deref(),
        Some("2026-01-01T00:45:00+00:00")
    );
}

#[test]
fn immediate_repair_snapshot_blocks_competing_document_and_item_writes() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("current.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);

    let mut owner = Connection::open(&current).unwrap();
    super::super::super::configure_sqlite_connection(&owner).unwrap();
    let transaction = owner
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let competitor = Connection::open(&current).unwrap();
    competitor.busy_timeout(Duration::from_millis(25)).unwrap();

    let document_error = competitor
        .execute(
            "INSERT INTO documents
               (id, title, raw_content, source, url, captured_at, created_at, updated_at)
             VALUES ('racing-doc', 'race', 'raw', 'fixture', 'fixture://race',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap_err();
    assert!(document_error.to_string().contains("database is locked"));
    let item_error = competitor
        .execute(
            "UPDATE items SET title = 'racing item' WHERE id = 'item'",
            [],
        )
        .unwrap_err();
    assert!(item_error.to_string().contains("database is locked"));
    drop(transaction);
}

#[test]
fn apply_schema_rejects_missing_ledger_pk_business_fks_and_bad_immutable_body() {
    let temp = TempDir::new().unwrap();

    let missing_pk = temp.path().join("missing-pk.db");
    seed_current(&missing_pk, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&missing_pk).unwrap();
    replace_ledger(
        &conn,
        "CREATE TABLE item_link_repair_ledger (
           item_id TEXT, target_document_id TEXT NOT NULL,
           evidence_sha256 TEXT NOT NULL, rule_version TEXT NOT NULL,
           applied_at TEXT NOT NULL
         );",
    );
    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();
    assert!(error.to_string().contains("not the primary key"));

    let business_fk = temp.path().join("business-fk.db");
    seed_current(&business_fk, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&business_fk).unwrap();
    replace_ledger(
        &conn,
        "CREATE TABLE item_link_repair_ledger (
           item_id TEXT PRIMARY KEY, target_document_id TEXT NOT NULL,
           evidence_sha256 TEXT NOT NULL, rule_version TEXT NOT NULL,
           applied_at TEXT NOT NULL,
           FOREIGN KEY (item_id) REFERENCES items(id) ON DELETE RESTRICT,
           FOREIGN KEY (target_document_id) REFERENCES documents(id) ON DELETE RESTRICT
         );",
    );
    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();
    assert!(error
        .to_string()
        .contains("must not reference mutable business rows"));

    let bad_trigger = temp.path().join("bad-trigger.db");
    seed_current(&bad_trigger, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&bad_trigger).unwrap();
    conn.execute_batch(
        "DROP TRIGGER item_link_repair_ledger_no_update;
         CREATE TRIGGER item_link_repair_ledger_no_update
         BEFORE UPDATE ON item_link_repair_ledger BEGIN SELECT 1; END;",
    )
    .unwrap();
    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();
    assert!(error.to_string().contains("unexpected body"));
}

#[test]
fn apply_schema_behavior_probe_is_rollback_only_when_guards_are_valid() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("valid-guard-probe.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    let before = validator_probe_counts(&conn);

    schema_validation::validate_apply_schema(&conn).unwrap();

    assert_eq!(validator_probe_counts(&conn), before);
}

#[test]
fn apply_schema_behavior_probe_rejects_noop_observation_update_without_pollution() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("noop-observation-update.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "DROP TRIGGER observations_require_document_update;
         CREATE TRIGGER observations_require_document_update
         BEFORE UPDATE OF item_type, document_id ON items
         WHEN 0 AND NEW.item_type = 'observation' AND NEW.document_id IS NULL
           AND NOT (
             OLD.item_type = 'observation' AND OLD.document_id IS NULL
           )
         BEGIN
           SELECT RAISE(ABORT, 'observation requires document_id');
         END;",
    )
    .unwrap();
    let before = validator_probe_counts(&conn);

    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();

    assert!(error
        .to_string()
        .contains("allowed a linked row to become detached"));
    assert_eq!(validator_probe_counts(&conn), before);
}

#[test]
fn apply_schema_behavior_probe_rejects_noop_observation_insert_without_pollution() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("noop-observation-insert.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "DROP TRIGGER observations_require_document_insert;
         CREATE TRIGGER observations_require_document_insert
         BEFORE INSERT ON items
         WHEN 0 AND NEW.item_type = 'observation' AND NEW.document_id IS NULL
         BEGIN
           SELECT RAISE(ABORT, 'observation requires document_id');
         END;",
    )
    .unwrap();
    let before = validator_probe_counts(&conn);

    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();

    assert!(error.to_string().contains("allowed a new detached row"));
    assert_eq!(validator_probe_counts(&conn), before);
}

#[test]
fn apply_schema_behavior_probe_rejects_noop_ledger_update_without_pollution() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("noop-ledger-update.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "DROP TRIGGER item_link_repair_ledger_no_update;
         CREATE TRIGGER item_link_repair_ledger_no_update
         BEFORE UPDATE ON item_link_repair_ledger WHEN 0 BEGIN
           SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
         END;",
    )
    .unwrap();
    let before = validator_probe_counts(&conn);

    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();

    assert!(error
        .to_string()
        .contains("ledger update guard is ineffective"));
    assert_eq!(validator_probe_counts(&conn), before);
}

#[test]
fn apply_schema_behavior_probe_rejects_noop_ledger_delete_without_pollution() {
    let temp = TempDir::new().unwrap();
    let current = temp.path().join("noop-ledger-delete.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "DROP TRIGGER item_link_repair_ledger_no_delete;
         CREATE TRIGGER item_link_repair_ledger_no_delete
         BEFORE DELETE ON item_link_repair_ledger WHEN 0 BEGIN
           SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
         END;",
    )
    .unwrap();
    let before = validator_probe_counts(&conn);

    let error = schema_validation::validate_apply_schema(&conn).unwrap_err();

    assert!(error
        .to_string()
        .contains("ledger delete guard is ineffective"));
    assert_eq!(validator_probe_counts(&conn), before);
}

fn validator_probe_counts(conn: &Connection) -> (i64, i64, i64, i64) {
    (
        conn.query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
            .unwrap(),
        conn.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .unwrap(),
        conn.query_row("SELECT COUNT(*) FROM item_link_repair_ledger", [], |row| {
            row.get(0)
        })
        .unwrap(),
        conn.query_row("SELECT COUNT(*) FROM items_fts", [], |row| row.get(0))
            .unwrap(),
    )
}

fn replace_ledger(conn: &Connection, create_table: &str) {
    conn.execute_batch(
        "DROP TRIGGER item_link_repair_ledger_no_update;
         DROP TRIGGER item_link_repair_ledger_no_delete;
         DROP TABLE item_link_repair_ledger;",
    )
    .unwrap();
    conn.execute_batch(create_table).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER item_link_repair_ledger_no_update
         BEFORE UPDATE ON item_link_repair_ledger BEGIN
           SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
         END;
         CREATE TRIGGER item_link_repair_ledger_no_delete
         BEFORE DELETE ON item_link_repair_ledger BEGIN
           SELECT RAISE(ABORT, 'item_link_repair_ledger is append-only');
         END;",
    )
    .unwrap();
}
