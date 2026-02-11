use crate::models::{ConversationRecord, EventRecord, ExtractionJobRecord};

pub trait ConversationRepository: Send + Sync {
    fn load_conversations(&self) -> Result<Vec<ConversationRecord>, String>;
    fn upsert_conversation(&self, record: &ConversationRecord) -> Result<(), String>;
}

pub trait JobRepository: Send + Sync {
    fn load_jobs(&self) -> Result<Vec<ExtractionJobRecord>, String>;
    fn upsert_job(&self, job: &ExtractionJobRecord) -> Result<(), String>;
}

pub trait EventRepository: Send + Sync {
    fn insert_event(&self, event: &EventRecord) -> Result<(), String>;
    fn event_counts_since(&self, since: Option<&str>) -> Result<Vec<(String, usize)>, String>;
}
