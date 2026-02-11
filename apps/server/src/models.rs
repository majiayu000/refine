use chrono::{DateTime, Utc};
use refine_core::knowledge::Item;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
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

#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub content: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub title: Option<String>,
    pub captured_at: Option<String>,
    pub idempotency_key: Option<String>,
    pub ingest_only: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateExtractionJobRequest {
    pub conversation_id: Option<String>,
    pub mode: Option<ExtractionMode>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub event_name: Option<String>,
    pub source: Option<String>,
    pub properties: Option<serde_json::Value>,
    pub occurred_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListItemsQuery {
    pub cursor: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ListConversationsQuery {
    pub status: Option<String>,
    pub cursor: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct RecommendationQuery {
    pub q: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct EventSummaryQuery {
    pub days: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CreateConversationResponse {
    pub conversation_id: String,
    pub status: ConversationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deduplicated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateExtractionJobResponse {
    pub job_id: String,
    pub status: JobStatus,
}

#[derive(Debug, Serialize)]
pub struct CreateEventResponse {
    pub event_id: String,
}

#[derive(Debug, Serialize)]
pub struct GetExtractionJobResponse {
    pub job: ExtractionJobRecord,
}

#[derive(Debug, Serialize)]
pub struct ItemDto {
    pub id: String,
    pub item_type: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
}

impl From<&Item> for ItemDto {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id().to_string(),
            item_type: item.item_type().as_str().to_string(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            tags: item
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_string())
                .collect(),
            created_at: item.created_at().to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ConversationDto {
    pub id: String,
    pub source: String,
    pub url: String,
    pub title: String,
    pub status: ConversationStatus,
    pub captured_at: String,
    pub created_at: String,
    pub preview: String,
}

impl From<&ConversationRecord> for ConversationDto {
    fn from(record: &ConversationRecord) -> Self {
        let title = record
            .title
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("(无标题)")
            .to_string();
        let mut preview = record.raw_content.trim().replace('\n', " ");
        if preview.chars().count() > 140 {
            preview = preview.chars().take(140).collect::<String>();
            preview.push_str("...");
        }

        Self {
            id: record.id.clone(),
            source: record.source.clone(),
            url: record.url.clone(),
            title,
            status: record.status.clone(),
            captured_at: record.captured_at.clone(),
            created_at: record.created_at.clone(),
            preview,
        }
    }
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
    use super::{
        CreateConversationResponse, CreateEventResponse, CreateExtractionJobResponse,
        ExtractionJobRecord, ExtractionMode, GetExtractionJobResponse, JobStatus,
    };
    use serde_json::json;

    #[test]
    fn create_conversation_response_omits_optional_fields_when_absent() {
        let value = serde_json::to_value(CreateConversationResponse {
            conversation_id: "c1".to_string(),
            status: super::ConversationStatus::Queued,
            deduplicated: None,
            job_id: None,
        })
        .expect("serialize response");

        assert_eq!(value, json!({"conversation_id":"c1","status":"queued"}));
    }

    #[test]
    fn create_conversation_response_includes_optional_fields_when_present() {
        let value = serde_json::to_value(CreateConversationResponse {
            conversation_id: "c1".to_string(),
            status: super::ConversationStatus::Captured,
            deduplicated: Some(true),
            job_id: Some("j1".to_string()),
        })
        .expect("serialize response");

        assert_eq!(
            value,
            json!({
                "conversation_id":"c1",
                "status":"captured",
                "deduplicated":true,
                "job_id":"j1"
            })
        );
    }

    #[test]
    fn extraction_job_responses_keep_snake_case_contract() {
        let create_value = serde_json::to_value(CreateExtractionJobResponse {
            job_id: "j1".to_string(),
            status: JobStatus::Pending,
        })
        .expect("serialize create response");
        assert_eq!(create_value, json!({"job_id":"j1","status":"pending"}));

        let job_record = ExtractionJobRecord {
            id: "j1".to_string(),
            conversation_id: "c1".to_string(),
            mode: ExtractionMode::Auto,
            status: JobStatus::Running,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:01Z".to_string(),
            error: None,
        };
        let get_value = serde_json::to_value(GetExtractionJobResponse { job: job_record })
            .expect("serialize get response");
        assert_eq!(get_value["job"]["id"], "j1");
        assert_eq!(get_value["job"]["status"], "running");
    }

    #[test]
    fn create_event_response_uses_event_id_field() {
        let value = serde_json::to_value(CreateEventResponse {
            event_id: "e1".to_string(),
        })
        .expect("serialize event response");
        assert_eq!(value, json!({"event_id":"e1"}));
    }
}
