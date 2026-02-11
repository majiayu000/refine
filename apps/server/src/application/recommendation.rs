use refine_core::search::SearchQuery as CoreSearchQuery;
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

use crate::application::error::ApplicationErrorCode;
use crate::state::AppState;

pub const RECOMMENDATION_MIN_QUERY_CHARS: usize = 10;

#[derive(Debug, Clone, Serialize)]
pub struct RecommendationMeta {
    pub latency_ms: u128,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendationItem {
    pub id: String,
    pub item_type: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub tags: Vec<String>,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendationResponse {
    pub triggered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_chars: Option<usize>,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    pub items: Vec<RecommendationItem>,
    pub meta: RecommendationMeta,
}

#[derive(Debug, Clone, Copy)]
struct RecommendationContext {
    strategy_name: &'static str,
    reason_name: &'static str,
}

#[derive(Debug, Clone)]
pub enum RecommendationError {
    Internal(String),
}

impl RecommendationError {
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

pub async fn recommend_items(
    state: Arc<AppState>,
    raw_query: Option<String>,
    raw_limit: Option<usize>,
) -> Result<RecommendationResponse, RecommendationError> {
    let keyword = normalize_query(raw_query);
    let limit = raw_limit.unwrap_or(5).clamp(1, 20);
    let latency_start = Instant::now();
    let context = recommendation_context(state.semantic_search_enabled);

    if keyword.chars().count() < RECOMMENDATION_MIN_QUERY_CHARS {
        return Ok(RecommendationResponse {
            triggered: false,
            reason: Some("query_too_short".to_string()),
            min_chars: Some(RECOMMENDATION_MIN_QUERY_CHARS),
            query: keyword,
            total: None,
            items: Vec::new(),
            meta: RecommendationMeta {
                latency_ms: latency_start.elapsed().as_millis(),
                strategy: context.strategy_name.to_string(),
            },
        });
    }

    let result = state
        .engine
        .search(CoreSearchQuery::new(&keyword).with_limit(limit))
        .await
        .map_err(|err| RecommendationError::Internal(err.to_string()))?;

    let items = result
        .items
        .iter()
        .map(|hit| RecommendationItem {
            id: hit.item.id().to_string(),
            item_type: hit.item.item_type().as_str().to_string(),
            title: hit.item.title().to_string(),
            summary: hit.item.summary().to_string(),
            content: hit.item.content().to_string(),
            tags: hit
                .item
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_string())
                .collect::<Vec<_>>(),
            score: hit.score,
            reason: context.reason_name.to_string(),
        })
        .collect::<Vec<_>>();

    Ok(RecommendationResponse {
        triggered: true,
        reason: None,
        min_chars: None,
        query: keyword,
        total: Some(result.total),
        items,
        meta: RecommendationMeta {
            latency_ms: latency_start.elapsed().as_millis(),
            strategy: context.strategy_name.to_string(),
        },
    })
}

fn recommendation_context(semantic_search_enabled: bool) -> RecommendationContext {
    if semantic_search_enabled {
        RecommendationContext {
            strategy_name: "hybrid_search",
            reason_name: "hybrid_match",
        }
    } else {
        RecommendationContext {
            strategy_name: "keyword_search",
            reason_name: "keyword_match",
        }
    }
}

fn normalize_query(raw_query: Option<String>) -> String {
    raw_query.unwrap_or_default().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_query, recommendation_context, RecommendationError,
        RECOMMENDATION_MIN_QUERY_CHARS,
    };
    use crate::application::error::ApplicationErrorCode;

    #[test]
    fn normalize_query_trims_whitespace() {
        assert_eq!(normalize_query(Some("  hello  ".to_string())), "hello");
        assert_eq!(normalize_query(None), "");
    }

    #[test]
    fn recommendation_context_changes_with_semantic_flag() {
        let keyword = recommendation_context(false);
        assert_eq!(keyword.strategy_name, "keyword_search");
        assert_eq!(keyword.reason_name, "keyword_match");

        let hybrid = recommendation_context(true);
        assert_eq!(hybrid.strategy_name, "hybrid_search");
        assert_eq!(hybrid.reason_name, "hybrid_match");
    }

    #[test]
    fn min_chars_guard_is_ten() {
        assert_eq!(RECOMMENDATION_MIN_QUERY_CHARS, 10);
    }

    #[test]
    fn recommendation_error_maps_to_internal_code() {
        let err = RecommendationError::Internal("search failed".to_string());
        assert_eq!(err.code(), ApplicationErrorCode::Internal);
        assert_eq!(err.message(), "search failed");
    }
}
