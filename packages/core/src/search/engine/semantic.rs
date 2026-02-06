use super::{SearchEngine, VectorSearch};
use crate::error::InfraResult;
use crate::knowledge::Item;
use crate::search::query::{SearchHit, SearchQuery, SearchResult};
use std::sync::Arc;

impl SearchEngine {
    pub(super) async fn semantic_search(
        &self,
        vs: &Arc<dyn VectorSearch>,
        query: &SearchQuery,
    ) -> InfraResult<SearchResult<Item>> {
        let request_limit = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit)
            .saturating_add(100);

        let similar = vs.search(&query.text, request_limit).await?;

        let mut hits = Vec::new();
        for (id, score) in similar {
            if let Some(item) = self
                .item_repo
                .find_by_id(&crate::knowledge::ItemId::from_str(&id))
                .await?
            {
                if Self::matches_filter(&item, &query.filter) {
                    hits.push(SearchHit::new(item, score));
                }
            }
        }

        let total = hits.len();
        let paginated_hits = Self::paginate(hits, query.pagination.offset, query.pagination.limit);

        Ok(SearchResult {
            items: paginated_hits,
            total,
            query: query.clone(),
        })
    }
}
