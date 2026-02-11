use refine_core::search::SearchQuery as CoreSearchQuery;
use serde::Serialize;
use std::sync::Arc;

use crate::application::error::ApplicationErrorCode;
use crate::models::{
    ConversationDto, ConversationRecord, ConversationStatus, ItemDto, ListConversationsQuery,
    ListItemsQuery, SearchQuery,
};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ListConversationsResult {
    pub conversations: Vec<ConversationDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

pub async fn list_conversations(
    state: Arc<AppState>,
    query: ListConversationsQuery,
) -> ListConversationsResult {
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let status_filter = query
        .status
        .map(|status| status.trim().to_ascii_lowercase())
        .filter(|status| !status.is_empty());

    let mut conversations: Vec<ConversationRecord> = {
        let guard = state.runtime.conversations.read().await;
        guard.values().cloned().collect()
    };
    if let Some(filter_status) = status_filter {
        conversations.retain(|record| conversation_status_name(&record.status) == filter_status);
    }
    conversations.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));

    let total = conversations.len();
    let conversations = conversations
        .into_iter()
        .skip(cursor)
        .take(limit)
        .map(|record| ConversationDto::from(&record))
        .collect::<Vec<_>>();
    let next_cursor = paginate_next_cursor(cursor, conversations.len(), total);

    ListConversationsResult {
        conversations,
        total,
        next_cursor,
    }
}

#[derive(Debug, Serialize)]
pub struct ListItemsResult {
    pub items: Vec<ItemDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SearchItemsResult {
    pub items: Vec<ItemDto>,
}

#[derive(Debug, Serialize)]
pub struct QuotaResult {
    pub limit: Option<usize>,
    pub used: usize,
    pub remaining: Option<usize>,
    pub exceeded: bool,
}

#[derive(Debug, Clone)]
pub enum QueryError {
    Internal(String),
}

impl QueryError {
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::Internal(_) => ApplicationErrorCode::Internal,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Internal(message) => message,
        }
    }
}

pub async fn list_items(
    state: Arc<AppState>,
    query: ListItemsQuery,
) -> Result<ListItemsResult, QueryError> {
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let total = state
        .store
        .count_items(None)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let items = state
        .store
        .find_recent(None, cursor, limit)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let next_cursor = paginate_next_cursor(cursor, items.len(), total);

    Ok(ListItemsResult {
        items: items.iter().map(ItemDto::from).collect::<Vec<_>>(),
        total,
        next_cursor,
    })
}

pub async fn search_items(
    state: Arc<AppState>,
    query: SearchQuery,
) -> Result<SearchItemsResult, QueryError> {
    let keyword = query.q.unwrap_or_default().trim().to_string();
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    if keyword.is_empty() {
        return Ok(SearchItemsResult { items: Vec::new() });
    }

    let result = state
        .engine
        .search(CoreSearchQuery::new(&keyword).with_limit(limit))
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    Ok(SearchItemsResult {
        items: result
            .items
            .iter()
            .map(|hit| ItemDto::from(&hit.item))
            .collect::<Vec<_>>(),
    })
}

pub async fn get_quota(state: Arc<AppState>) -> Result<QuotaResult, QueryError> {
    let used = state
        .store
        .count_items(None)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    if state.free_quota_items == 0 {
        return Ok(QuotaResult {
            limit: None,
            used,
            remaining: None,
            exceeded: false,
        });
    }

    Ok(QuotaResult {
        limit: Some(state.free_quota_items),
        used,
        remaining: Some(state.free_quota_items.saturating_sub(used)),
        exceeded: used >= state.free_quota_items,
    })
}

fn conversation_status_name(status: &ConversationStatus) -> &'static str {
    match status {
        ConversationStatus::Captured => "captured",
        ConversationStatus::Queued => "queued",
        ConversationStatus::Processing => "processing",
        ConversationStatus::Processed => "processed",
        ConversationStatus::Failed => "failed",
    }
}

fn paginate_next_cursor(cursor: usize, returned: usize, total: usize) -> Option<usize> {
    if cursor + returned < total {
        Some(cursor + returned)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{conversation_status_name, paginate_next_cursor};
    use crate::models::ConversationStatus;

    #[test]
    fn paginate_next_cursor_returns_none_on_last_page() {
        assert_eq!(paginate_next_cursor(10, 5, 15), None);
        assert_eq!(paginate_next_cursor(0, 0, 0), None);
    }

    #[test]
    fn paginate_next_cursor_returns_next_when_remaining() {
        assert_eq!(paginate_next_cursor(0, 20, 55), Some(20));
        assert_eq!(paginate_next_cursor(20, 20, 55), Some(40));
    }

    #[test]
    fn conversation_status_name_is_stable_for_filters() {
        assert_eq!(
            conversation_status_name(&ConversationStatus::Captured),
            "captured"
        );
        assert_eq!(
            conversation_status_name(&ConversationStatus::Queued),
            "queued"
        );
        assert_eq!(
            conversation_status_name(&ConversationStatus::Processing),
            "processing"
        );
        assert_eq!(
            conversation_status_name(&ConversationStatus::Processed),
            "processed"
        );
        assert_eq!(
            conversation_status_name(&ConversationStatus::Failed),
            "failed"
        );
    }
}
