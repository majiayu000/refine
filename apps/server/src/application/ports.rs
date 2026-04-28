use crate::models::{ConversationRecord, EventRecord, ExtractionJobRecord};

pub trait ConversationRepository: Send + Sync {
    fn find_conversation_by_id(&self, id: &str) -> Result<Option<ConversationRecord>, String>;
    fn list_conversations(
        &self,
        status: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, String>;
    fn count_conversations(&self, status: Option<&str>) -> Result<usize, String>;
    fn upsert_conversation(&self, record: &ConversationRecord) -> Result<(), String>;
    /// Atomically inserts `record` or, on idempotency-key conflict, returns the
    /// pre-existing row unchanged. Callers detect deduplication by comparing
    /// the returned `id` with the one they supplied in `record`.
    fn insert_or_fetch_conversation_by_idempotency(
        &self,
        record: &ConversationRecord,
    ) -> Result<ConversationRecord, String>;
}

pub trait JobRepository: Send + Sync {
    fn find_job_by_id(&self, id: &str) -> Result<Option<ExtractionJobRecord>, String>;
    fn upsert_job(&self, job: &ExtractionJobRecord) -> Result<(), String>;
}

pub trait EventRepository: Send + Sync {
    fn insert_event(&self, event: &EventRecord) -> Result<(), String>;
    fn event_counts_since(&self, since: Option<&str>) -> Result<Vec<(String, usize)>, String>;
}
