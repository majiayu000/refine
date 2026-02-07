pub mod commands;
mod dto;
mod state;

use refine_core::infra::SqliteStore;
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use state::AppState;

pub fn build_state() -> AppState {
    let db_path = get_db_path();
    ensure_db_dir(&db_path);

    let sqlite_store = Arc::new(SqliteStore::open(&db_path).expect("打开数据库失败"));
    let store: Arc<dyn ItemRepository> = sqlite_store;
    let engine = Arc::new(SearchEngine::new(store.clone()));

    AppState { store, engine }
}

fn get_db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("refine")
        .join("data.db")
}

fn ensure_db_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}
