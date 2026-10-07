//! 文档仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::InfraResult;
use crate::knowledge::{
    Document, DocumentId, Item, SessionProjectionContext, SessionProjectionMetadata,
    SessionProjectionRevision, SessionProjectionVersion,
};
use async_trait::async_trait;

#[async_trait]
pub trait DocumentRepository: Send + Sync {
    async fn find_by_id(&self, id: &DocumentId) -> InfraResult<Option<Document>>;
    async fn find_by_url(&self, url: &str) -> InfraResult<Option<Document>>;
    async fn find_recent(&self, offset: usize, limit: usize) -> InfraResult<Vec<Document>>;
    async fn find_items_by_document_id(&self, id: &DocumentId) -> InfraResult<Vec<Item>>;
    async fn count(&self) -> InfraResult<usize>;
    async fn save(&self, doc: &Document) -> InfraResult<()>;
    async fn save_with_replaced_items(&self, doc: &Document, items: &[Item]) -> InfraResult<()>;
    async fn save_with_replaced_items_and_delete_documents(
        &self,
        doc: &Document,
        items: &[Item],
        source_document_ids: &[DocumentId],
        obsolete_document_ids: &[DocumentId],
    ) -> InfraResult<()>;
    /// Publish a validated extraction and its independent recipe identity in
    /// the same transaction. Previous observations remain in revision history.
    async fn save_session_projection(
        &self,
        doc: &Document,
        items: &[Item],
        source_document_ids: &[DocumentId],
        obsolete_document_ids: &[DocumentId],
        metadata: &SessionProjectionMetadata,
    ) -> InfraResult<()>;
    async fn find_session_projection_versions(&self) -> InfraResult<Vec<SessionProjectionVersion>>;
    /// Read source, items, current evidence and recent history consistently.
    async fn find_session_projection_context(
        &self,
        session_ref: &str,
    ) -> InfraResult<Option<SessionProjectionContext>>;
    async fn find_session_projection_history(
        &self,
        document_id: &DocumentId,
        limit: usize,
    ) -> InfraResult<Vec<SessionProjectionRevision>>;
    async fn delete_documents_with_items(&self, document_ids: &[DocumentId]) -> InfraResult<()>;
    async fn delete(&self, id: &DocumentId) -> InfraResult<bool>;
    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Document>>;
    async fn count_text_hits(&self, query: &str) -> InfraResult<usize>;
}
