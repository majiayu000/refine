use refine_core::infra::{DocumentDetailDto, DocumentDto};
use refine_core::search::SearchQuery as CoreSearchQuery;
use serde::Serialize;
use std::sync::Arc;

use crate::application::error::ApplicationErrorCode;
use crate::models::{
    ConversationDto, ItemDto, ListConversationsQuery, ListItemsQuery, SearchQuery,
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
) -> Result<ListConversationsResult, QueryError> {
    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let status_filter = query
        .status
        .map(|status| status.trim().to_ascii_lowercase())
        .filter(|status| !status.is_empty());

    let total = state
        .conversation_repo
        .count_conversations(status_filter.as_deref())
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;
    let conversations = state
        .conversation_repo
        .list_conversations(status_filter.as_deref(), cursor, limit)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    let mapped = conversations
        .iter()
        .map(ConversationDto::from)
        .collect::<Vec<_>>();
    let next_cursor = paginate_next_cursor(cursor, mapped.len(), total);

    Ok(ListConversationsResult {
        conversations: mapped,
        total,
        next_cursor,
    })
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
    NotFound(String),
    Internal(String),
}

impl QueryError {
    pub fn code(&self) -> ApplicationErrorCode {
        match self {
            Self::NotFound(_) => ApplicationErrorCode::NotFound,
            Self::Internal(_) => ApplicationErrorCode::Internal,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotFound(message) => message,
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

pub async fn get_quota(state: Arc<AppState>, user_id: &str) -> Result<QuotaResult, QueryError> {
    let used = state
        .store
        .count_items(None)
        .await
        .map_err(|err| QueryError::Internal(err.to_string()))?;

    if state.free_quota_items == 0 || state.is_premium_user(user_id) {
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

#[derive(Debug, Serialize)]
pub struct ListDocumentsResult {
    pub documents: Vec<DocumentDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

pub async fn list_documents(
    state: Arc<AppState>,
    cursor: usize,
    limit: usize,
) -> Result<ListDocumentsResult, QueryError> {
    let limit = limit.clamp(1, 100);
    let total = state
        .doc_store
        .count()
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;
    let docs = state
        .doc_store
        .find_recent(cursor, limit)
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;
    let item_counts = count_items_per_document(&state, &docs).await?;
    let next_cursor = paginate_next_cursor(cursor, docs.len(), total);

    Ok(ListDocumentsResult {
        documents: docs
            .iter()
            .enumerate()
            .map(|(i, doc)| DocumentDto {
                id: doc.id().to_string(),
                title: doc.title().unwrap_or("(无标题)").to_string(),
                source: doc.source().to_string(),
                url: doc.url().to_string(),
                item_count: item_counts.get(i).copied().unwrap_or(0),
                captured_at: doc.captured_at().to_rfc3339(),
                created_at: doc.created_at().to_rfc3339(),
            })
            .collect(),
        total,
        next_cursor,
    })
}

pub async fn get_document(
    state: Arc<AppState>,
    doc_id: &str,
) -> Result<DocumentDetailDto, QueryError> {
    let doc = state
        .doc_store
        .find_by_id(&refine_core::knowledge::DocumentId::from(doc_id))
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?
        .ok_or_else(|| QueryError::NotFound("Document not found".to_string()))?;

    let items = state
        .store
        .find_by_document_id(&refine_core::knowledge::DocumentId::from(doc_id))
        .await
        .map_err(|e| QueryError::Internal(e.to_string()))?;

    Ok(DocumentDetailDto {
        id: doc.id().to_string(),
        title: doc.title().unwrap_or("(无标题)").to_string(),
        raw_content: doc.raw_content().to_string(),
        source: doc.source().to_string(),
        url: doc.url().to_string(),
        captured_at: doc.captured_at().to_rfc3339(),
        created_at: doc.created_at().to_rfc3339(),
        items: items.iter().map(ItemDto::from).collect(),
    })
}

async fn count_items_per_document(
    state: &Arc<AppState>,
    docs: &[refine_core::knowledge::Document],
) -> Result<Vec<usize>, QueryError> {
    let mut counts = Vec::with_capacity(docs.len());
    for doc in docs {
        let items = state
            .store
            .find_by_document_id(doc.id())
            .await
            .map_err(|e| QueryError::Internal(e.to_string()))?;
        counts.push(items.len());
    }
    Ok(counts)
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
    use super::paginate_next_cursor;

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
}
