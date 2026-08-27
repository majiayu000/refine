use super::*;
use rusqlite::params;
use tempfile::TempDir;

fn ts(milliseconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(milliseconds).unwrap()
}

fn evidence_item(group: &str, item: &str, title: &str, milliseconds: i64) -> EvidenceItem {
    EvidenceItem {
        id: item.into(),
        title: title.into(),
        created_at: ts(milliseconds),
        document_id: group.into(),
    }
}

fn current_document(id: &str, title: &str, milliseconds: i64) -> CurrentDocument {
    CurrentDocument {
        id: id.into(),
        title: title.into(),
        created_at: ts(milliseconds),
    }
}

fn detached_items(ids: &[&str]) -> HashMap<String, CurrentItem> {
    ids.iter()
        .map(|id| {
            (
                (*id).into(),
                CurrentItem {
                    is_observation: true,
                    document_id: None,
                },
            )
        })
        .collect()
}

#[test]
fn exact_rule_rejects_ambiguous_late_missing_linked_and_conflicting_groups() {
    let ambiguous = build_plan_from_rows(
        vec![evidence_item("g", "i", "same", 1_000)],
        vec![
            current_document("d1", "same", 1_000),
            current_document("d2", "same", 1_000),
        ],
        detached_items(&["i"]),
        "hash".into(),
    );
    assert_eq!(ambiguous.stats.candidate_groups, 0);
    assert_eq!(ambiguous.stats.ambiguous_groups, 1);

    let late = build_plan_from_rows(
        vec![evidence_item("g", "i", "summary", 1_000)],
        vec![current_document("d", "summary", 2_001)],
        detached_items(&["i"]),
        "hash".into(),
    );
    assert_eq!(late.stats.candidate_groups, 0);
    assert_eq!(late.stats.unproven_groups, 1);

    let missing_document = build_plan_from_rows(
        vec![evidence_item("g", "i", "summary", 1_000)],
        vec![],
        detached_items(&["i"]),
        "hash".into(),
    );
    assert_eq!(missing_document.stats.candidate_groups, 0);

    let mut linked = detached_items(&["i"]);
    linked.get_mut("i").unwrap().document_id = Some("already".into());
    let linked = build_plan_from_rows(
        vec![evidence_item("g", "i", "summary", 1_000)],
        vec![current_document("d", "summary", 1_000)],
        linked,
        "hash".into(),
    );
    assert_eq!(linked.stats.already_linked_items, 1);
    assert_eq!(linked.stats.candidate_items, 0);

    let conflicts = build_plan_from_rows(
        vec![
            evidence_item("g1", "i1", "same", 1_000),
            evidence_item("g2", "i2", "same", 1_000),
        ],
        vec![current_document("d", "same", 1_000)],
        detached_items(&["i1", "i2"]),
        "hash".into(),
    );
    assert_eq!(conflicts.stats.target_conflicts, 2);
    assert_eq!(conflicts.stats.candidate_groups, 0);
}

#[test]
fn programmatic_known_scale_fixture_matches_pinned_counts() {
    let mut evidence = Vec::with_capacity(63_776);
    let mut documents = Vec::with_capacity(7_885);
    let mut current_items = HashMap::with_capacity(63_776);
    for group in 0..7_885 {
        let title = format!("summary-{group}");
        let document_id = format!("document-{group}");
        documents.push(current_document(&document_id, &title, group as i64));
        let item_count = if group < 696 { 9 } else { 8 };
        for item_index in 0..item_count {
            let item_id = format!("item-{group}-{item_index}");
            let item_title = if item_index == 0 {
                title.clone()
            } else {
                format!("detail-{group}-{item_index}")
            };
            evidence.push(evidence_item(
                &format!("shadow-{group}"),
                &item_id,
                &item_title,
                group as i64,
            ));
            current_items.insert(
                item_id,
                CurrentItem {
                    is_observation: true,
                    document_id: None,
                },
            );
        }
    }

    let plan = build_plan_from_rows(evidence, documents, current_items, "hash".into());
    assert_eq!(plan.stats.candidate_groups, 7_885);
    assert_eq!(plan.stats.candidate_items, 63_776);
    assert_eq!(plan.stats.target_conflicts, 0);
}

#[test]
fn evidence_hash_and_symlink_fail_closed() {
    let temp = TempDir::new().unwrap();
    let evidence = temp.path().join("evidence.db");
    seed_evidence(&evidence, &[("shadow", vec![("item", "summary")])]);
    let current = temp.path().join("current.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);

    let mismatch = plan_repair(&current, &evidence, &"0".repeat(64)).unwrap_err();
    assert!(mismatch.to_string().contains("SHA-256 mismatch"));

    #[cfg(unix)]
    {
        let link = temp.path().join("evidence-link.db");
        std::os::unix::fs::symlink(&evidence, &link).unwrap();
        let hash = hash_file(&evidence).unwrap();
        let error = plan_repair(&current, &link, &hash).unwrap_err();
        assert!(error.to_string().contains("non-symlink"));
    }
}

#[test]
fn apply_is_transactional_backed_up_append_only_and_idempotent() {
    let temp = TempDir::new().unwrap();
    let evidence = temp.path().join("evidence.db");
    seed_evidence(&evidence, &[("shadow", vec![("item", "summary")])]);
    let current = temp.path().join("current.db");
    seed_current(&current, &[("doc", "summary", vec!["item"])]);
    let hash = hash_file(&evidence).unwrap();
    let backup = temp.path().join("before-repair.db");

    let dry_run_one = plan_repair(&current, &evidence, &hash).unwrap();
    let dry_run_two = plan_repair(&current, &evidence, &hash).unwrap();
    assert_eq!(dry_run_one.stats, dry_run_two.stats);
    assert_eq!(dry_run_one.stats.candidate_items, 1);

    let first = apply_repair(&current, &evidence, &hash, &backup).unwrap();
    assert_eq!(first.changed_items, 1);
    assert_eq!(first.ledger_rows_added, 1);
    assert_eq!(
        audit_detached_observations(&current)
            .unwrap()
            .detached_observations,
        0
    );
    assert_eq!(
        audit_detached_observations(&backup)
            .unwrap()
            .detached_observations,
        1
    );

    let conn = Connection::open(&current).unwrap();
    assert_eq!(
        scalar_count(&conn, "SELECT COUNT(*) FROM item_link_repair_ledger").unwrap(),
        1
    );
    assert!(conn
        .execute(
            "UPDATE item_link_repair_ledger SET rule_version = 'changed'",
            [],
        )
        .unwrap_err()
        .to_string()
        .contains("append-only"));
    assert!(conn
        .execute("DELETE FROM item_link_repair_ledger", [])
        .unwrap_err()
        .to_string()
        .contains("append-only"));
    assert!(conn
        .execute("UPDATE items SET document_id = NULL WHERE id = 'item'", [])
        .unwrap_err()
        .to_string()
        .contains("observation requires document_id"));
    drop(conn);

    let second = apply_repair(&current, &evidence, &hash, &backup).unwrap();
    assert_eq!(second.changed_items, 0);
    assert_eq!(second.ledger_rows_added, 0);
    let conn = Connection::open(&current).unwrap();
    assert_eq!(
        scalar_count(&conn, "SELECT COUNT(*) FROM item_link_repair_ledger").unwrap(),
        1
    );
}

#[test]
fn backup_failure_and_update_failure_leave_current_database_unchanged() {
    let temp = TempDir::new().unwrap();
    let evidence = temp.path().join("evidence.db");
    seed_evidence(
        &evidence,
        &[("shadow", vec![("item-1", "summary"), ("item-2", "detail")])],
    );
    let current = temp.path().join("current.db");
    seed_current(&current, &[("doc", "summary", vec!["item-1", "item-2"])]);
    let hash = hash_file(&evidence).unwrap();

    let existing_backup = temp.path().join("existing.db");
    std::fs::write(&existing_backup, b"occupied").unwrap();
    let backup_error = apply_repair(&current, &evidence, &hash, &existing_backup).unwrap_err();
    assert!(backup_error.to_string().contains("already exists"));
    assert_eq!(
        audit_detached_observations(&current)
            .unwrap()
            .detached_observations,
        2
    );

    let conn = Connection::open(&current).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER fail_second_repair BEFORE UPDATE ON items
         WHEN NEW.id = 'item-2'
         BEGIN SELECT RAISE(ABORT, 'fixture update failure'); END;",
    )
    .unwrap();
    drop(conn);
    let transaction_backup = temp.path().join("transaction-backup.db");
    let update_error = apply_repair(&current, &evidence, &hash, &transaction_backup).unwrap_err();
    assert!(update_error.to_string().contains("fixture update failure"));
    assert_eq!(
        audit_detached_observations(&current)
            .unwrap()
            .detached_observations,
        2
    );
    let conn = Connection::open(&current).unwrap();
    assert_eq!(
        scalar_count(&conn, "SELECT COUNT(*) FROM item_link_repair_ledger").unwrap(),
        0
    );
}

fn seed_evidence(path: &Path, groups: &[(&str, Vec<(&str, &str)>)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE documents (id TEXT PRIMARY KEY);
         CREATE TABLE items (
           id TEXT PRIMARY KEY, item_type TEXT NOT NULL, title TEXT NOT NULL,
           created_at TEXT NOT NULL, document_id TEXT
         );",
    )
    .unwrap();
    for (group, items) in groups {
        for (item_id, title) in items {
            conn.execute(
                "INSERT INTO items (id, item_type, title, created_at, document_id)
                 VALUES (?1, 'observation', ?2, '2026-01-01T00:00:00.500Z', ?3)",
                params![item_id, title, group],
            )
            .unwrap();
        }
    }
}

fn seed_current(path: &Path, groups: &[(&str, &str, Vec<&str>)]) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE documents (
           id TEXT PRIMARY KEY, title TEXT, raw_content TEXT NOT NULL,
           source TEXT NOT NULL, url TEXT NOT NULL, captured_at TEXT NOT NULL,
           created_at TEXT NOT NULL, updated_at TEXT NOT NULL
         );
         CREATE TABLE items (
           id TEXT PRIMARY KEY, item_type TEXT NOT NULL, title TEXT NOT NULL,
           summary TEXT NOT NULL, content TEXT NOT NULL, tags TEXT NOT NULL,
           source TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
           document_id TEXT, excerpt TEXT
         );",
    )
    .unwrap();
    for (document_id, title, items) in groups {
        conn.execute(
            "INSERT INTO documents
               (id, title, raw_content, source, url, captured_at, created_at, updated_at)
             VALUES (?1, ?2, 'raw', 'fixture', ?3, '2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            params![document_id, title, format!("fixture://{document_id}")],
        )
        .unwrap();
        for item_id in items {
            let item_title = if item_id.ends_with("1") || *item_id == "item" {
                *title
            } else {
                "detail"
            };
            conn.execute(
                "INSERT INTO items
                   (id, item_type, title, summary, content, tags, source,
                    created_at, updated_at, document_id, excerpt)
                 VALUES (?1, 'observation', ?2, ?2, '', '[]', NULL,
                         '2026-01-01T00:00:00.500Z', '2026-01-01T00:00:00.500Z', NULL, NULL)",
                params![item_id, item_title],
            )
            .unwrap();
        }
    }
    super::super::prepare_sqlite_db(&conn).unwrap();

    let insert_error = conn
        .execute(
            "INSERT INTO items
               (id, item_type, title, summary, content, tags, created_at, updated_at)
             VALUES ('new-detached', 'observation', 'bad', 'bad', '', '[]',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap_err();
    assert!(insert_error
        .to_string()
        .contains("observation requires document_id"));
}
