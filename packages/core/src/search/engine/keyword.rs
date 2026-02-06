use super::SearchEngine;
use crate::error::InfraResult;
use crate::knowledge::Item;
use crate::search::query::{SearchFilter, SearchQuery, SearchResult};

const KEYWORD_SCAN_BATCH: usize = 128;

impl SearchEngine {
    pub(super) async fn keyword_search(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        let has_filter = query.filter.item_type.is_some() || !query.filter.tags.is_empty();

        if !has_filter {
            let total = self.item_repo.count_text_hits(&query.text).await?;
            let items = self
                .item_repo
                .search_text(&query.text, query.pagination.offset, query.pagination.limit)
                .await?;

            return Ok(SearchResult {
                items: Self::to_hits(items),
                total,
                query: query.clone(),
            });
        }

        let mut total = 0usize;
        let mut raw_offset = 0usize;
        let mut page_items = Vec::new();
        let page_end = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit);

        loop {
            let batch = self
                .item_repo
                .search_text(&query.text, raw_offset, KEYWORD_SCAN_BATCH)
                .await?;

            if batch.is_empty() {
                break;
            }

            let batch_len = batch.len();
            raw_offset = raw_offset.saturating_add(batch_len);

            for item in batch {
                if !Self::matches_filter(&item, &query.filter) {
                    continue;
                }

                if total >= query.pagination.offset && total < page_end {
                    page_items.push(item);
                }
                total = total.saturating_add(1);
            }

            if batch_len < KEYWORD_SCAN_BATCH {
                break;
            }
        }

        Ok(SearchResult {
            items: Self::to_hits(page_items),
            total,
            query: query.clone(),
        })
    }

    pub(super) async fn get_recent(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        if query.filter.tags.is_empty() {
            let total = self.item_repo.count_items(query.filter.item_type).await?;
            let items = self
                .item_repo
                .find_recent(
                    query.filter.item_type,
                    query.pagination.offset,
                    query.pagination.limit,
                )
                .await?;

            return Ok(SearchResult {
                items: Self::to_hits(items),
                total,
                query: query.clone(),
            });
        }

        let all_items = if let Some(item_type) = query.filter.item_type {
            self.item_repo.find_by_type(item_type).await?
        } else {
            self.item_repo.find_all().await?
        };

        let filtered_items: Vec<Item> = all_items
            .into_iter()
            .filter(|item| Self::matches_filter(item, &query.filter))
            .collect();

        let total = filtered_items.len();
        let paginated_items = Self::paginate(
            filtered_items,
            query.pagination.offset,
            query.pagination.limit,
        );

        Ok(SearchResult {
            items: Self::to_hits(paginated_items),
            total,
            query: query.clone(),
        })
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
