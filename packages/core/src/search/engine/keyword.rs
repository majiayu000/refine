use super::SearchEngine;
use crate::error::InfraResult;
use crate::knowledge::Item;
use crate::search::query::{SearchFilter, SearchQuery, SearchResult};

impl SearchEngine {
    pub(super) async fn keyword_search(
        &self,
        query: &SearchQuery,
    ) -> InfraResult<SearchResult<Item>> {
        let (items, total) = self
            .item_repo
            .search_page(
                &query.text,
                query.filter.item_type,
                &query.filter.tags,
                query.pagination.offset,
                query.pagination.limit,
            )
            .await?;
        Ok(SearchResult {
            items: Self::to_hits(items),
            total,
            query: query.clone(),
        })
    }

    pub(super) async fn get_recent(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        self.keyword_search(query).await
    }

    pub(super) fn matches_filter(item: &Item, filter: &SearchFilter) -> bool {
        if let Some(item_type) = filter.item_type {
            if item.item_type() != item_type {
                return false;
            }
        }

        if filter.tags.is_empty() {
            return true;
        }

        let item_tags: std::collections::HashSet<String> = item
            .tags()
            .iter()
            .map(|tag| tag.as_str().to_lowercase())
            .collect();

        filter
            .tags
            .iter()
            .all(|tag| item_tags.contains(&tag.to_lowercase()))
    }

    pub(super) fn paginate<T>(items: Vec<T>, offset: usize, limit: usize) -> Vec<T> {
        items.into_iter().skip(offset).take(limit).collect()
    }
}
