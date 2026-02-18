pub mod commands;
mod dto;
mod state;

use refine_core::infra::{ensure_db_dir, resolve_db_path, SqliteStore};
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::sync::Arc;

pub use state::AppState;

pub fn build_state() -> AppState {
    let db_path = resolve_db_path(&["REFINE_DESKTOP_DB_PATH"]);
    ensure_db_dir(&db_path).expect("无法创建数据库目录");

    let sqlite_store = Arc::new(SqliteStore::open(&db_path).expect("打开数据库失败"));
    let store: Arc<dyn ItemRepository> = sqlite_store;
    let engine = Arc::new(SearchEngine::new(store.clone()));

    AppState { store, engine }
}
