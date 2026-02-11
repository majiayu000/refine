use axum::http::StatusCode;
use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::models::{normalize_timestamp, CreateEventRequest, EventRecord};
use crate::state::AppState;

const FUNNEL_EVENTS: [&str; 5] = [
    "conversation_extracted",
    "conversation_synced",
    "recommendation_exposed",
    "recommendation_clicked",
    "knowledge_reused",
];

#[derive(Debug, Clone, Serialize)]
pub struct CreateEventResult {
    pub event_id: String,
}

#[derive(Debug, Clone)]
pub enum CreateEventError {
    BadRequest(String),
    Internal(String),
}

impl CreateEventError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(message) => message,
            Self::Internal(message) => message,
        }
    }
}

pub fn create_event(
    state: Arc<AppState>,
    user_id: String,
    payload: CreateEventRequest,
) -> Result<CreateEventResult, CreateEventError> {
    let event_name = payload
        .event_name
        .and_then(|value| trim_optional(&value).map(ToString::to_string))
        .ok_or_else(|| CreateEventError::BadRequest("event_name is required".to_string()))?;

    let source = payload
        .source
        .as_deref()
        .and_then(trim_optional)
        .unwrap_or("unknown")
        .to_string();
    let properties = normalize_event_properties(payload.properties);

    let event = EventRecord {
        id: Uuid::new_v4().to_string(),
        user_id,
        event_name,
        source,
        properties,
        created_at: normalize_timestamp(payload.occurred_at),
    };
    state
        .persistence
        .insert_event(&event)
        .map_err(CreateEventError::Internal)?;

    Ok(CreateEventResult { event_id: event.id })
}

#[derive(Debug, Clone, Serialize)]
pub struct EventSummaryResult {
    pub days: u32,
    pub since: String,
    pub counts: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum EventSummaryError {
    Internal(String),
}

impl EventSummaryError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Internal(message) => message,
        }
    }
}

pub fn get_event_summary(
    state: Arc<AppState>,
    requested_days: Option<u32>,
) -> Result<EventSummaryResult, EventSummaryError> {
    let days = normalize_days(requested_days);
    let since = (Utc::now() - Duration::days(days as i64)).to_rfc3339();
    let pairs = state
        .persistence
        .event_counts_since(Some(&since))
        .map_err(EventSummaryError::Internal)?;

    let mut counts = serde_json::Map::new();
    for name in FUNNEL_EVENTS {
        counts.insert(name.to_string(), json!(0));
    }
    for (name, count) in pairs {
        counts.insert(name, json!(count));
    }

    Ok(EventSummaryResult {
        days,
        since,
        counts: serde_json::Value::Object(counts),
    })
}

fn normalize_days(requested_days: Option<u32>) -> u32 {
    requested_days.unwrap_or(7).clamp(1, 90)
}

fn normalize_event_properties(raw: Option<serde_json::Value>) -> serde_json::Value {
    match raw {
        Some(serde_json::Value::Object(map)) => serde_json::Value::Object(map),
        _ => json!({}),
    }
}

fn trim_optional(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_days, normalize_event_properties};
    use serde_json::json;

    #[test]
    fn normalize_days_clamps_to_safe_range() {
        assert_eq!(normalize_days(None), 7);
        assert_eq!(normalize_days(Some(0)), 1);
        assert_eq!(normalize_days(Some(91)), 90);
        assert_eq!(normalize_days(Some(30)), 30);
    }

    #[test]
    fn normalize_event_properties_keeps_only_object_values() {
        assert_eq!(normalize_event_properties(Some(json!({"k": "v"}))), json!({"k": "v"}));
        assert_eq!(normalize_event_properties(Some(json!(123))), json!({}));
        assert_eq!(normalize_event_properties(None), json!({}));
    }
}
