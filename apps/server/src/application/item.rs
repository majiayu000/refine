use axum::http::StatusCode;
use refine_core::knowledge::ItemId;
use serde::Serialize;
use std::sync::Arc;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct DeleteItemResult {
    pub deleted: bool,
    pub id: String,
}

#[derive(Debug, Clone)]
pub enum DeleteItemError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl DeleteItemError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(message) => message,
            Self::NotFound(message) => message,
            Self::Internal(message) => message,
        }
    }
}

pub async fn delete_item(
    state: Arc<AppState>,
    item_id: String,
) -> Result<DeleteItemResult, DeleteItemError> {
    let normalized_id = normalize_item_id(item_id)?;

    let deleted = state
        .store
        .delete(&ItemId::from_str(&normalized_id))
        .await
        .map_err(|err| DeleteItemError::Internal(err.to_string()))?;
    if !deleted {
        return Err(DeleteItemError::NotFound("Item not found".to_string()));
    }

    if let Err(err) = state.engine.remove_from_index(&normalized_id).await {
        tracing::warn!(
            "item {} removed from store but failed to remove from vector index: {}",
            normalized_id,
            err
        );
    }

    Ok(DeleteItemResult {
        deleted: true,
        id: normalized_id,
    })
}

fn normalize_item_id(item_id: String) -> Result<String, DeleteItemError> {
    let normalized_id = item_id.trim().to_string();
    if normalized_id.is_empty() {
        return Err(DeleteItemError::BadRequest("item_id is required".to_string()));
    }
    Ok(normalized_id)
}

#[cfg(test)]
mod tests {
    use super::{normalize_item_id, DeleteItemError};
    use axum::http::StatusCode;

    #[test]
    fn normalize_item_id_rejects_blank_input() {
        let err = normalize_item_id("   ".to_string()).expect_err("blank id should fail");
        assert!(matches!(err, DeleteItemError::BadRequest(_)));
        assert_eq!(err.message(), "item_id is required");
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn normalize_item_id_trims_whitespace() {
        let id = normalize_item_id("  abc  ".to_string()).expect("id should be valid");
        assert_eq!(id, "abc");
    }
}
