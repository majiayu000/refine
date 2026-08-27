use crate::error::{InfraError, InfraResult};
use rusqlite::Connection;

pub(super) fn migrate_items_document_fk(conn: &Connection) -> InfraResult<()> {
    if has_fail_closed_document_fk(conn)? {
        return Ok(());
    }

    let before = LinkIntegritySnapshot::read(conn)?;

    conn.execute_batch(
        r#"
        DROP TRIGGER IF EXISTS items_ai;
        DROP TRIGGER IF EXISTS items_ad;
        DROP TRIGGER IF EXISTS items_au;
        UPDATE items
        SET document_id = NULL
        WHERE document_id IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM documents WHERE documents.id = items.document_id
          );
        ALTER TABLE items RENAME TO items_without_document_fk;
        CREATE TABLE items (
            id TEXT PRIMARY KEY,
            item_type TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            content TEXT NOT NULL,
            tags TEXT NOT NULL,
            source TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            document_id TEXT,
            excerpt TEXT,
            FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE RESTRICT
        );
        INSERT INTO items
          (rowid, id, item_type, title, summary, content, tags, source,
           created_at, updated_at, document_id, excerpt)
        SELECT rowid, id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt
        FROM items_without_document_fk;
        DROP TABLE items_without_document_fk;
        CREATE INDEX IF NOT EXISTS idx_items_type ON items(item_type);
        CREATE INDEX IF NOT EXISTS idx_items_created ON items(created_at);
        CREATE INDEX IF NOT EXISTS idx_items_document ON items(document_id);
        CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
            INSERT INTO items_fts(rowid, title, summary, content, tags)
            VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
            VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
            INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
            VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
            INSERT INTO items_fts(rowid, title, summary, content, tags)
            VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
        END;
        "#,
    )
    .map_err(|e| InfraError::Database(e.to_string()))?;

    verify_migration(conn, before)
}

fn has_fail_closed_document_fk(conn: &Connection) -> InfraResult<bool> {
    let mut stmt = conn
        .prepare("PRAGMA foreign_key_list(items)")
        .map_err(|e| InfraError::Database(e.to_string()))?;
    let fks = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| InfraError::Database(e.to_string()))?;

    for fk in fks {
        let (target_table, source_column, target_column, delete_action) =
            fk.map_err(|e| InfraError::Database(e.to_string()))?;
        if target_table == "documents"
            && source_column == "document_id"
            && target_column == "id"
            && (delete_action.eq_ignore_ascii_case("RESTRICT")
                || delete_action.eq_ignore_ascii_case("NO ACTION"))
        {
            return Ok(true);
        }
    }

    Ok(false)
}

#[derive(Clone, Copy)]
struct LinkIntegritySnapshot {
    items: i64,
    documents: i64,
    valid_links: i64,
    null_or_dangling: i64,
}

impl LinkIntegritySnapshot {
    fn read(conn: &Connection) -> InfraResult<Self> {
        Ok(Self {
            items: query_scalar_count(conn, "SELECT COUNT(*) FROM items")?,
            documents: query_scalar_count(conn, "SELECT COUNT(*) FROM documents")?,
            valid_links: query_scalar_count(
                conn,
                "SELECT COUNT(*) FROM items i
                 WHERE i.document_id IS NOT NULL
                   AND EXISTS (SELECT 1 FROM documents d WHERE d.id = i.document_id)",
            )?,
            null_or_dangling: query_scalar_count(
                conn,
                "SELECT COUNT(*) FROM items i
                 WHERE i.document_id IS NULL
                    OR NOT EXISTS (SELECT 1 FROM documents d WHERE d.id = i.document_id)",
            )?,
        })
    }
}

fn verify_migration(conn: &Connection, before: LinkIntegritySnapshot) -> InfraResult<()> {
    let after = LinkIntegritySnapshot::read(conn)?;
    let nulls_after =
        query_scalar_count(conn, "SELECT COUNT(*) FROM items WHERE document_id IS NULL")?;
    let trigger_count = query_scalar_count(
        conn,
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'trigger' AND name IN ('items_ai', 'items_ad', 'items_au')",
    )?;
    let fts_table_count = query_scalar_count(
        conn,
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name = 'items_fts'",
    )?;
    let foreign_key_violations = query_scalar_count(
        conn,
        "SELECT COUNT(*) FROM pragma_foreign_key_check('items')",
    )?;

    if after.items != before.items
        || after.documents != before.documents
        || after.valid_links != before.valid_links
        || nulls_after != before.null_or_dangling
        || trigger_count != 3
        || fts_table_count != 1
        || foreign_key_violations != 0
        || !has_fail_closed_document_fk(conn)?
    {
        return Err(InfraError::Database(format!(
            "items document FK migration failed integrity checks: items {}->{}, documents {}->{}, valid_links {}->{}, expected_nulls {}, actual_nulls {}, triggers {}, fts_tables {}, fk_violations {}",
            before.items,
            after.items,
            before.documents,
            after.documents,
            before.valid_links,
            after.valid_links,
            before.null_or_dangling,
            nulls_after,
            trigger_count,
            fts_table_count,
            foreign_key_violations
        )));
    }

    Ok(())
}

fn query_scalar_count(conn: &Connection, sql: &str) -> InfraResult<i64> {
    conn.query_row(sql, [], |row| row.get(0))
        .map_err(|e| InfraError::Database(e.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::infra::{configure_sqlite_connection, prepare_sqlite_db};
    use rusqlite::Connection;

    #[test]
    fn fresh_schema_uses_fail_closed_document_fk() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        configure_sqlite_connection(&conn).expect("configure database");
        prepare_sqlite_db(&conn).expect("prepare fresh database");

        assert_eq!(
            items_document_delete_action(&conn).as_deref(),
            Some("RESTRICT")
        );
        assert_eq!(foreign_key_violation_count(&conn), 0);
    }

    #[test]
    fn set_null_fk_upgrade_preserves_links_nulls_fts_and_conversation_references() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        configure_sqlite_connection(&conn).expect("configure database");
        seed_set_null_schema(&conn);

        prepare_sqlite_db(&conn).expect("upgrade SET NULL document FK");

        assert_eq!(
            items_document_delete_action(&conn).as_deref(),
            Some("RESTRICT")
        );
        assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM documents"), 1);
        assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM items"), 2);
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items WHERE document_id = 'doc-linked'"
            ),
            1
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items WHERE document_id IS NULL"
            ),
            1
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items_fts WHERE items_fts MATCH 'linked'"
            ),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT item_ids FROM conversations WHERE id = 'conv-linked'",
                [],
                |row| row.get::<_, String>(0)
            )
            .expect("read conversation references"),
            "[\"item-linked\",\"item-null\"]"
        );

        conn.execute_batch(
            "INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
             VALUES
              ('item-fresh', 'knowledge', 'Fresh trigger', 'S', 'C', '[]', NULL,
               '2026-01-03T00:00:00Z', '2026-01-03T00:00:00Z', 'doc-linked', NULL);",
        )
        .expect("insert through recreated FTS trigger");
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items_fts WHERE items_fts MATCH 'fresh'"
            ),
            1
        );

        let delete_error = conn
            .execute("DELETE FROM documents WHERE id = 'doc-linked'", [])
            .expect_err("upgraded FK must reject direct parent deletion");
        assert!(delete_error
            .to_string()
            .contains("FOREIGN KEY constraint failed"));
        assert_eq!(scalar(&conn, "SELECT COUNT(*) FROM documents"), 1);
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items WHERE document_id = 'doc-linked'"
            ),
            2
        );
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM items WHERE document_id IS NULL"
            ),
            1
        );
        assert_eq!(foreign_key_violation_count(&conn), 0);
    }

    #[test]
    fn pre_fk_dangling_observation_is_normalized_before_guards_are_installed() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        configure_sqlite_connection(&conn).expect("configure database");
        seed_dangling_observation_without_fk(&conn);

        prepare_sqlite_db(&conn).expect("upgrade dangling pre-FK observation");

        assert_eq!(
            conn.query_row(
                "SELECT document_id FROM items WHERE id = 'dangling-observation'",
                [],
                |row| row.get::<_, Option<String>>(0)
            )
            .expect("read normalized observation"),
            None
        );
        assert_eq!(
            items_document_delete_action(&conn).as_deref(),
            Some("RESTRICT")
        );
        assert_eq!(foreign_key_violation_count(&conn), 0);
        assert_eq!(
            scalar(
                &conn,
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger' AND name IN (
                   'observations_require_document_insert',
                   'observations_require_document_update'
                 )"
            ),
            2
        );

        let error = conn
            .execute(
                "INSERT INTO items
                   (id, item_type, title, summary, content, tags, source,
                    created_at, updated_at, document_id, excerpt)
                 VALUES
                   ('new-detached', 'observation', 'New', 'New', '', '[]', 'test',
                    '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', NULL, NULL)",
                [],
            )
            .expect_err("post-migration guard must reject new detached observation");
        assert!(error
            .to_string()
            .contains("observation requires document_id"));
    }

    fn items_document_delete_action(conn: &Connection) -> Option<String> {
        conn.query_row(
            "SELECT on_delete FROM pragma_foreign_key_list('items')
             WHERE \"from\" = 'document_id' AND \"table\" = 'documents'",
            [],
            |row| row.get(0),
        )
        .ok()
    }

    fn foreign_key_violation_count(conn: &Connection) -> i64 {
        scalar(
            conn,
            "SELECT COUNT(*) FROM pragma_foreign_key_check('items')",
        )
    }

    fn scalar(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |row| row.get(0))
            .unwrap_or_else(|err| panic!("query failed: {err}; sql={sql}"))
    }

    fn seed_set_null_schema(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE documents (
                id TEXT PRIMARY KEY,
                title TEXT,
                raw_content TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE items (
                id TEXT PRIMARY KEY,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                document_id TEXT,
                excerpt TEXT,
                FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE SET NULL
            );
            CREATE INDEX idx_items_type ON items(item_type);
            CREATE INDEX idx_items_created ON items(created_at);
            CREATE INDEX idx_items_document ON items(document_id);
            CREATE VIRTUAL TABLE items_fts USING fts5(
                title, summary, content, tags, content=items, content_rowid=rowid
            );
            CREATE TRIGGER items_ai AFTER INSERT ON items BEGIN
                INSERT INTO items_fts(rowid, title, summary, content, tags)
                VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
            END;
            CREATE TRIGGER items_ad AFTER DELETE ON items BEGIN
                INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
                VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
            END;
            CREATE TRIGGER items_au AFTER UPDATE ON items BEGIN
                INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
                VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
                INSERT INTO items_fts(rowid, title, summary, content, tags)
                VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
            END;
            CREATE TABLE conversations (
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
            INSERT INTO documents
              (id, title, raw_content, source, url, captured_at, created_at, updated_at)
            VALUES
              ('doc-linked', 'Document', 'raw', 'claude', 'https://example.com/linked',
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
            INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
            VALUES
              ('item-linked', 'knowledge', 'Linked item', 'S', 'C', '[]', NULL,
               '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'doc-linked', NULL),
              ('item-null', 'knowledge', 'Legacy null', 'S', 'C', '[]', NULL,
               '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z', NULL, NULL);
            INSERT INTO conversations
              (id, user_id, source, url, title, raw_content, metadata_json,
               captured_at, created_at, status, idempotency_key, item_ids, last_error)
            VALUES
              ('conv-linked', 'test', 'claude', 'https://example.com/conversation', NULL,
               'raw', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
               'processed', 'conv-linked-key', '["item-linked","item-null"]', NULL);
            PRAGMA user_version = 1;
            "#,
        )
        .expect("seed SET NULL schema");
    }

    fn seed_dangling_observation_without_fk(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE documents (
                id TEXT PRIMARY KEY,
                title TEXT,
                raw_content TEXT NOT NULL,
                source TEXT NOT NULL,
                url TEXT NOT NULL,
                captured_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE items (
                id TEXT PRIMARY KEY,
                item_type TEXT NOT NULL,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                source TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                document_id TEXT,
                excerpt TEXT
            );
            INSERT INTO items
              (id, item_type, title, summary, content, tags, source,
               created_at, updated_at, document_id, excerpt)
            VALUES
              ('dangling-observation', 'observation', 'Legacy observation', 'S', 'C',
               '[]', 'legacy', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
               'missing-document', NULL);
            PRAGMA user_version = 1;
            "#,
        )
        .expect("seed pre-FK dangling observation schema");
    }
}
