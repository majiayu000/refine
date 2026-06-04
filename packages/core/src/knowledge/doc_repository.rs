//! 文档仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::RepoResult;
use crate::knowledge::{Document, DocumentId, Item};
use async_trait::async_trait;

#[async_trait]
pub trait DocumentRepository: Send + Sync {
    async fn find_by_id(&self, id: &DocumentId) -> RepoResult<Option<Document>>;
    async fn find_by_url(&self, url: &str) -> RepoResult<Option<Document>>;
    async fn find_recent(&self, offset: usize, limit: usize) -> RepoResult<Vec<Document>>;
    async fn count(&self) -> RepoResult<usize>;
    async fn save(&self, doc: &Document) -> RepoResult<()>;
    async fn save_with_replaced_items(&self, doc: &Document, items: &[Item]) -> RepoResult<()>;
    async fn delete(&self, id: &DocumentId) -> RepoResult<bool>;
    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> RepoResult<Vec<Document>>;
    async fn count_text_hits(&self, query: &str) -> RepoResult<usize>;
}
