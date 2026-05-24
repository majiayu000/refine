use super::{SearchEngine, VectorSearch};
use crate::error::InfraResult;
use crate::knowledge::{Item, ItemId};
use crate::search::query::{SearchHit, SearchQuery, SearchResult};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

const HYBRID_CANDIDATE_BUFFER: usize = 100;
const KEYWORD_WEIGHT: f32 = 0.45;
const VECTOR_WEIGHT: f32 = 0.55;
const VECTOR_RANK_BONUS_WEIGHT: f32 = 0.10;

impl SearchEngine {
    pub(super) async fn hybrid_search(
        &self,
        vs: &Arc<dyn VectorSearch>,
        query: &SearchQuery,
    ) -> InfraResult<SearchResult<Item>> {
        let request_limit = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit)
            .saturating_add(HYBRID_CANDIDATE_BUFFER);

        let keyword_query = query.clone().with_offset(0).with_limit(request_limit);
        let keyword_result = self.keyword_search(&keyword_query).await?;
        let semantic = vs
            .search(&query.text, request_limit)
            .await
            ?;

        let mut merged: HashMap<String, SearchHit<Item>> = HashMap::new();

        for (rank, hit) in keyword_result.items.into_iter().enumerate() {
            let score = KEYWORD_WEIGHT * rank_score(rank);
            merge_hit(&mut merged, SearchHit::new(hit.item, score));
        }

        for (rank, (id, raw_score)) in semantic.into_iter().enumerate() {
            let Some(item) = self
                .item_repo
                .find_by_id(&ItemId::from(id.as_str()))
                .await?
            else {
                continue;
            };
            if !Self::matches_filter(&item, &query.filter) {
                continue;
            }

            let score =
                VECTOR_WEIGHT * unit_score(raw_score) + VECTOR_RANK_BONUS_WEIGHT * rank_score(rank);
            merge_hit(&mut merged, SearchHit::new(item, score));
        }

        let mut hits = merged.into_values().collect::<Vec<_>>();
        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
        });

        let total = hits.len();
        let paginated_hits = Self::paginate(hits, query.pagination.offset, query.pagination.limit);

        Ok(SearchResult {
            items: paginated_hits,
            total,
            query: query.clone(),
        })
    }
}

fn merge_hit(target: &mut HashMap<String, SearchHit<Item>>, incoming: SearchHit<Item>) {
    let id = incoming.item.id().to_string();
    if let Some(existing) = target.get_mut(&id) {
        existing.score += incoming.score;
        return;
    }
    target.insert(id, incoming);
}

fn rank_score(rank: usize) -> f32 {
    1.0 / ((rank + 1) as f32).sqrt()
}

fn unit_score(score: f32) -> f32 {
    if !score.is_finite() {
        return 0.0;
    }
    score.clamp(0.0, 1.0)
}
