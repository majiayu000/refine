//! 文档仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::InfraResult;
use crate::knowledge::{Document, DocumentId};
use async_trait::async_trait;

#[async_trait]
pub trait DocumentRepository: Send + Sync {
    async fn find_by_id(&self, id: &DocumentId) -> InfraResult<Option<Document>>;
    async fn find_by_url(&self, url: &str) -> InfraResult<Option<Document>>;
    async fn find_recent(&self, offset: usize, limit: usize) -> InfraResult<Vec<Document>>;
    async fn count(&self) -> InfraResult<usize>;
    async fn save(&self, doc: &Document) -> InfraResult<()>;
    async fn delete(&self, id: &DocumentId) -> InfraResult<bool>;
    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Document>>;
    async fn count_text_hits(&self, query: &str) -> InfraResult<usize>;
}
