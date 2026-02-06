use refine_core::infra::SqliteStore;
use refine_core::search::SearchEngine;
use std::sync::Arc;

/// 应用状态
pub struct AppState {
    pub store: Arc<SqliteStore>,
    pub engine: Arc<SearchEngine>,
}
