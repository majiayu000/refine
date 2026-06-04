use async_trait::async_trait;

use crate::error::InfraResult;

use super::record::{ConversationRecord, EventRecord, ExtractionJobRecord};

#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn find_conversation_by_id(&self, id: &str) -> InfraResult<Option<ConversationRecord>>;
    async fn list_conversations(
        &self,
        status: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<ConversationRecord>>;
    async fn count_conversations(&self, status: Option<&str>) -> InfraResult<usize>;
    async fn upsert_conversation(&self, record: &ConversationRecord) -> InfraResult<()>;
    /// Atomically inserts `record` or, on idempotency-key conflict, returns the
    /// pre-existing row unchanged. Callers detect deduplication by comparing
    /// the returned `id` with the one they supplied in `record`.
    async fn insert_or_fetch_conversation_by_idempotency(
        &self,
        record: &ConversationRecord,
    ) -> InfraResult<ConversationRecord>;
}

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn find_job_by_id(&self, id: &str) -> InfraResult<Option<ExtractionJobRecord>>;
    async fn upsert_job(&self, job: &ExtractionJobRecord) -> InfraResult<()>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert_event(&self, event: &EventRecord) -> InfraResult<()>;
    async fn event_counts_since(&self, since: Option<&str>) -> InfraResult<Vec<(String, usize)>>;
}
