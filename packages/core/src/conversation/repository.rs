use async_trait::async_trait;

use crate::error::InfraResult;
use crate::knowledge::{Document, Item};

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
    /// Atomically inserts a conversation and its initial extraction job. On an
    /// idempotency conflict, returns the existing conversation and its newest
    /// recoverable job, creating the supplied job for that conversation when
    /// an earlier partial write left it without one.
    async fn insert_or_fetch_conversation_with_job(
        &self,
        record: &ConversationRecord,
        job: &ExtractionJobRecord,
    ) -> InfraResult<(ConversationRecord, Option<ExtractionJobRecord>)>;
}

#[async_trait]
pub trait JobRepository: Send + Sync {
    async fn find_job_by_id(&self, id: &str) -> InfraResult<Option<ExtractionJobRecord>>;
    async fn upsert_job(&self, job: &ExtractionJobRecord) -> InfraResult<()>;
    /// Atomically queues the parent conversation and inserts `job`. If the
    /// conversation already has a pending or running job, returns that job.
    async fn enqueue_job(&self, job: &ExtractionJobRecord) -> InfraResult<ExtractionJobRecord>;
    /// Converges legacy crash states where the conversation was already
    /// committed as processed before its active job reached succeeded.
    async fn reconcile_processed_jobs(&self, now: &str) -> InfraResult<usize>;
    async fn list_recoverable_jobs(
        &self,
        now: &str,
        limit: usize,
    ) -> InfraResult<Vec<ExtractionJobRecord>>;
    async fn claim_job(
        &self,
        id: &str,
        owner: &str,
        now: &str,
        lease_expires_at: &str,
    ) -> InfraResult<Option<ExtractionJobRecord>>;
    async fn renew_job_lease(
        &self,
        id: &str,
        owner: &str,
        now: &str,
        lease_expires_at: &str,
    ) -> InfraResult<bool>;
    async fn finish_job_claim(
        &self,
        id: &str,
        owner: &str,
        status: super::record::JobStatus,
        item_ids: &[String],
        error: Option<&str>,
        now: &str,
    ) -> InfraResult<bool>;
    /// Atomically verifies the active lease, replaces the extracted document
    /// items, and marks both job and conversation successful.
    async fn finish_job_claim_with_results(
        &self,
        id: &str,
        owner: &str,
        document: &Document,
        items: &[Item],
        now: &str,
    ) -> InfraResult<bool>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert_event(&self, event: &EventRecord) -> InfraResult<()>;
    async fn event_counts_since(&self, since: Option<&str>) -> InfraResult<Vec<(String, usize)>>;
}
