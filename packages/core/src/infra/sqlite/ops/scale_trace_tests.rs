//! Measure the actual SQL entry point separately from end-to-end timings.
use super::{init_schema, search_page};
use crate::infra::sqlite::rows::configure_connection;
use crate::knowledge::ItemType;
use rusqlite::Connection;
use std::cell::RefCell;

thread_local! {
    static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn record_statement(sql: &str) {
    STATEMENTS.with(|statements| statements.borrow_mut().push(sql.to_string()));
}

fn trace_page(conn: &mut Connection, rows: usize, query: &str, tags: &[String]) {
    STATEMENTS.with(|statements| statements.borrow_mut().clear());
    conn.trace(Some(record_statement));
    let (items, total) = search_page(conn, query, Some(ItemType::Knowledge), tags, 0, 20)
        .expect("actual filtered page");
    conn.trace(None);
    assert_eq!(items.len(), 20);
    assert_eq!(total, rows);
    let statements = STATEMENTS.with(|statements| std::mem::take(&mut *statements.borrow_mut()));
    // SQLite labels virtual-table implementation statements with "--". Keep
    // their count visible, but do not confuse them with repeated page queries.
    let top_level: Vec<_> = statements
        .iter()
        .filter(|sql| !sql.trim_start().starts_with("--"))
        .collect();
    assert_eq!(top_level.len(), 4, "query={query:?}: {top_level:#?}");
    assert_eq!(top_level[0].trim(), "BEGIN DEFERRED");
    assert!(top_level[1].contains("SELECT COUNT(*)"));
    assert!(top_level[2].contains("SELECT i.id"));
    assert_eq!(top_level[3].trim(), "COMMIT");
    eprintln!(
        "search_sql_trace={}",
        serde_json::json!({
            "rows": rows, "query": query, "tags": tags,
            "top_level_statements": top_level.len(), "select_statements": top_level.len() - 2,
            "transaction_statements": 2,
            "sqlite_internal_statements": statements.len() - top_level.len(),
            "materialized_items": items.len(), "total": total,
            "measurement_boundary": "production ops::search_page with rusqlite trace"
        })
    );
}

fn seeded_store(rows: usize) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    configure_connection(&conn, true).unwrap();
    init_schema(&conn).unwrap();
    conn.execute(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?1)
         INSERT INTO items(id, item_type, title, summary, content, tags, created_at, updated_at)
         SELECT printf('%06d', n), 'knowledge', 'rust backend 灰度发布', '部署', 'rust async',
                '[\"backend\",\"数据库\"]', '2026-10-06T00:00:00Z', '2026-10-06T00:00:00Z' FROM seq",
        [rows],
    )
    .unwrap();
    conn
}

#[test]
fn filtered_page_uses_one_count_and_one_page_statement() {
    let mut conn = seeded_store(1_000);
    for query in ["rust", "部署", "部署 rust", "灰度发布", ""] {
        trace_page(
            &mut conn,
            1_000,
            query,
            &["backend".into(), "数据库".into()],
        );
    }
}

#[test]
#[ignore = "informational SQL trace across three database sizes"]
fn filtered_search_sql_scale_probe() {
    for rows in [1_000, 10_000, 100_000] {
        let mut conn = seeded_store(rows);
        for query in ["rust", "部署", "部署 rust", "灰度发布", ""] {
            trace_page(&mut conn, rows, query, &["backend".into(), "数据库".into()]);
        }
    }
}
