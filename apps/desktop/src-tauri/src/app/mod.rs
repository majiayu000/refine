pub mod commands;
mod dto;
mod state;

use refine_core::infra::{
    ensure_db_dir, migrate_stale_dbs, resolve_db_path, MigrationReport, SqliteStore,
};
use refine_core::knowledge::{DocumentRepository, ItemRepository};
use refine_core::search::SearchEngine;
use std::sync::Arc;

pub use state::AppState;

pub fn build_state() -> AppState {
    let db_path = resolve_db_path(&["REFINE_DESKTOP_DB_PATH"]);
    ensure_db_dir(&db_path).expect("无法创建数据库目录");
    match migrate_stale_dbs(&db_path) {
        Ok(MigrationReport::NoOp) => {}
        Ok(MigrationReport::Migrated {
            sources,
            rows_copied,
        }) => {
            eprintln!(
                "[refine] migrated {} row(s) from legacy DB(s): {}",
                rows_copied,
                sources
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        Err(e) => eprintln!("[refine] warning: DB migration failed (continuing): {e}"),
    }

    let sqlite_store = Arc::new(SqliteStore::open(&db_path).expect("打开数据库失败"));
    let store: Arc<dyn ItemRepository> = sqlite_store.clone();
    let doc_store: Arc<dyn DocumentRepository> = sqlite_store;
    let engine = Arc::new(SearchEngine::new(store.clone()));

    AppState {
        store,
        doc_store,
        engine,
    }
}
