use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchEngine;
use std::sync::Arc;

/// 应用状态
pub struct AppState {
    pub store: Arc<dyn ItemRepository>,
    pub engine: Arc<SearchEngine>,
}
