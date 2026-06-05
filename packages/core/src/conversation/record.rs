use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Captured,
    Queued,
    Processing,
    Processed,
    Failed,
}

impl ConversationStatus {
    pub fn can_transition_to(&self, next: &Self) -> bool {
        if self == next {
            return true;
        }

        matches!(
            (self, next),
            (Self::Captured, Self::Queued)
                | (Self::Queued, Self::Processing)
                | (Self::Queued, Self::Failed)
                | (Self::Processing, Self::Processed)
                | (Self::Processing, Self::Failed)
                | (Self::Failed, Self::Queued)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    pub fn can_transition_to(&self, next: &Self) -> bool {
        if self == next {
            return true;
        }

        matches!(
            (self, next),
            (Self::Pending, Self::Running)
                | (Self::Running, Self::Succeeded)
                | (Self::Running, Self::Failed)
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMode {
    Auto,
    Knowledge,
    Skill,
    Snippet,
}

impl ExtractionMode {
    pub fn from_option(mode: Option<ExtractionMode>) -> Self {
        mode.unwrap_or(Self::Auto)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    pub id: String,
    pub user_id: String,
    pub source: String,
    pub url: String,
    pub title: Option<String>,
    pub raw_content: String,
    pub metadata: serde_json::Value,
    pub captured_at: String,
    pub created_at: String,
    pub status: ConversationStatus,
    pub idempotency_key: String,
    pub item_ids: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionJobRecord {
    pub id: String,
    pub conversation_id: String,
    pub mode: ExtractionMode,
    pub status: JobStatus,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub user_id: String,
    pub event_name: String,
    pub source: String,
    pub properties: serde_json::Value,
    pub created_at: String,
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn normalize_timestamp(raw: Option<String>) -> String {
    match raw {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .map(|ts| ts.with_timezone(&Utc).to_rfc3339())
            .unwrap_or_else(|_| now_iso()),
        None => now_iso(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ConversationStatus, JobStatus};

    #[test]
    fn conversation_status_rejects_terminal_regression() {
        assert!(ConversationStatus::Captured.can_transition_to(&ConversationStatus::Queued));
        assert!(ConversationStatus::Queued.can_transition_to(&ConversationStatus::Processing));
        assert!(ConversationStatus::Processing.can_transition_to(&ConversationStatus::Processed));
        assert!(ConversationStatus::Failed.can_transition_to(&ConversationStatus::Queued));
        assert!(!ConversationStatus::Processed.can_transition_to(&ConversationStatus::Queued));
    }

    #[test]
    fn job_status_rejects_terminal_regression() {
        assert!(JobStatus::Pending.can_transition_to(&JobStatus::Running));
        assert!(JobStatus::Running.can_transition_to(&JobStatus::Succeeded));
        assert!(JobStatus::Running.can_transition_to(&JobStatus::Failed));
        assert!(!JobStatus::Succeeded.can_transition_to(&JobStatus::Pending));
        assert!(!JobStatus::Failed.can_transition_to(&JobStatus::Running));
    }
}
