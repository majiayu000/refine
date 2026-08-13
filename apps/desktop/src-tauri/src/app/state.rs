use refine_core::knowledge::{DocumentRepository, ItemRepository};
use refine_core::search::SearchEngine;
use std::sync::Arc;

/// 应用状态
pub struct AppState {
    pub store: Arc<dyn ItemRepository>,
    pub doc_store: Arc<dyn DocumentRepository>,
    pub engine: Arc<SearchEngine>,
}
