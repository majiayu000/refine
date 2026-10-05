use super::prepare_migration_state;
use crate::infra::startup_tests::assert_overlapping_initializers;

#[test]
fn overlapping_old_state_upgrades_recheck_under_the_write_lock() {
    assert_overlapping_initializers(
        |conn| {
            conn.execute_batch(
                "CREATE TABLE refine_legacy_migration_state (
                source_path TEXT PRIMARY KEY,
                signature TEXT NOT NULL,
                migrated_at TEXT NOT NULL
            )",
            )
            .unwrap()
        },
        prepare_migration_state,
        "ALTER TABLE refine_legacy_migration_state",
        |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('refine_legacy_migration_state') WHERE name='content_hash'",
                [], |row| row.get(0)
            ).unwrap();
            assert_eq!(count, 1);
            prepare_migration_state(conn).unwrap();
        },
    );
}
