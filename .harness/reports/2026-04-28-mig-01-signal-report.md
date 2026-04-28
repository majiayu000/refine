# MIG-01 Signal Report

## Scope

- Issue: stale-DB migration can drop server-owned data during startup.
- Affected path: `apps/server/src/state.rs` startup flow with `packages/core/src/infra/db_migration.rs`.

## Observed Execution Order

1. `AppState::build()` resolves `refine.db` and calls `migrate_stale_dbs(&db_path)`.
2. `migrate_stale_dbs()` initializes only the core schema from `packages/core/src/infra/schema.sql`.
3. `copy_all_tables()` copies only legacy tables already present in the target database.
4. `ServerPersistence::new(db_path)` runs later and creates `conversations`, `extraction_jobs`, and `events`.

## Root Cause

`packages/core/src/infra/db_migration.rs` intentionally skips any legacy table missing from the target database. In the real server startup order, the server-owned tables do not exist yet when migration runs. A legacy `server.db` can therefore contain `conversations` and `events`, but those rows are skipped, and the legacy file is still renamed to `.migrated`.

## Expected Fix Shape

- Ensure the server schema exists before stale-DB migration runs in the server startup path.
- Keep the fix local to the server startup flow and add a regression test that mirrors that order.
