use crate::knowledge::Item;
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: &str = "1.0";
pub const CONTRACT_VERSION_HEADER: &str = "x-refine-contract-version";

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedConversationInput {
    pub content: String,
    pub url: String,
    pub source: String,
    pub title: Option<String>,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize)]
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

pub fn trim_optional(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn trim_required_field(value: Option<String>, field_name: &str) -> Result<String, String> {
    value
        .and_then(|value| trim_optional(value.as_str()).map(ToString::to_string))
        .ok_or_else(|| format!("Missing required field: {}", field_name))
}

pub fn normalize_conversation_input(
    content: Option<String>,
    url: Option<String>,
    source: Option<String>,
    title: Option<String>,
    idempotency_key: Option<String>,
) -> Result<NormalizedConversationInput, String> {
    Ok(NormalizedConversationInput {
        content: trim_required_field(content, "content")?,
        url: trim_required_field(url, "url")?,
        source: trim_required_field(source, "source")?,
        title: title.and_then(|value| trim_optional(value.as_str()).map(ToString::to_string)),
        idempotency_key: trim_required_field(idempotency_key, "idempotency_key")?,
    })
}

pub fn normalize_contract_major(version: &str) -> &str {
    let version = version.trim();
    version.split('.').next().unwrap_or(version)
}

pub fn is_contract_compatible(client_version: &str, server_version: &str) -> bool {
    let client_major = normalize_contract_major(client_version);
    let server_major = normalize_contract_major(server_version);
    !client_major.is_empty() && client_major == server_major
}

#[cfg(test)]
mod tests {
    use super::{
        is_contract_compatible, normalize_contract_major, normalize_conversation_input,
        trim_optional, trim_required_field, CreateConversationRequest, ItemDto,
        NormalizedConversationInput,
    };
    use crate::knowledge::Item;
    use serde_json::json;

    #[test]
    fn normalize_contract_major_uses_first_segment() {
        assert_eq!(normalize_contract_major("1.2.3"), "1");
        assert_eq!(normalize_contract_major(" 2 "), "2");
    }

    #[test]
    fn is_contract_compatible_checks_major_version() {
        assert!(is_contract_compatible("1.0", "1.2.0"));
        assert!(is_contract_compatible("1.9", "1.0"));
        assert!(!is_contract_compatible("2.0", "1.0"));
        assert!(!is_contract_compatible("", "1.0"));
    }

    #[test]
    fn trim_helpers_normalize_required_fields() {
        assert_eq!(trim_optional(" value "), Some("value"));
        assert_eq!(trim_optional("  "), None);
        assert_eq!(
            trim_required_field(Some(" x ".to_string()), "content"),
            Ok("x".to_string())
        );
        assert_eq!(
            trim_required_field(None, "content"),
            Err("Missing required field: content".to_string())
        );
    }

    #[test]
    fn normalize_conversation_input_trims_and_validates_fields() {
        let normalized = normalize_conversation_input(
            Some(" content ".to_string()),
            Some(" url ".to_string()),
            Some(" source ".to_string()),
            Some(" title ".to_string()),
            Some(" key ".to_string()),
        )
        .expect("should normalize valid payload");
        assert_eq!(
            normalized,
            NormalizedConversationInput {
                content: "content".to_string(),
                url: "url".to_string(),
                source: "source".to_string(),
                title: Some("title".to_string()),
                idempotency_key: "key".to_string(),
            }
        );
    }

    #[test]
    fn normalize_conversation_input_reports_missing_required_field() {
        let err = normalize_conversation_input(None, Some("url".to_string()), None, None, None)
            .expect_err("should fail for missing content");
        assert_eq!(err, "Missing required field: content");
    }

    #[test]
    fn create_conversation_request_deserializes_optional_fields() {
        let payload = json!({
            "content": "c",
            "url": "u",
            "source": "s",
            "title": "t",
            "idempotency_key": "k"
        });
        let parsed: CreateConversationRequest =
            serde_json::from_value(payload).expect("payload should parse");
        assert_eq!(parsed.content.as_deref(), Some("c"));
        assert_eq!(parsed.url.as_deref(), Some("u"));
        assert_eq!(parsed.source.as_deref(), Some("s"));
        assert_eq!(parsed.title.as_deref(), Some("t"));
        assert_eq!(parsed.idempotency_key.as_deref(), Some("k"));
    }

    #[test]
    fn item_dto_uses_item_type_string_contract() {
        let item = Item::new_knowledge("title", "summary");
        let dto = ItemDto::from(&item);
        assert_eq!(dto.item_type, "knowledge");
        assert_eq!(dto.title, "title");
    }
}
