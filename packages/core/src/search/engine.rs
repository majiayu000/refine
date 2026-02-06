//! 搜索引擎
//!
//! 组合关键词搜索和语义搜索

use crate::error::InfraResult;
use crate::knowledge::{Item, ItemRepository};
use crate::search::query::{SearchFilter, SearchHit, SearchQuery, SearchResult};
use async_trait::async_trait;
use std::sync::Arc;

const KEYWORD_SCAN_BATCH: usize = 128;

/// 向量搜索接口
#[async_trait]
pub trait VectorSearch: Send + Sync {
    /// 语义搜索
    async fn search(&self, query: &str, limit: usize) -> InfraResult<Vec<(String, f32)>>;

    /// 索引文档
    async fn index(&self, id: &str, text: &str) -> InfraResult<()>;

    /// 删除文档
    async fn remove(&self, id: &str) -> InfraResult<()>;
}

/// 搜索引擎
pub struct SearchEngine {
    item_repo: Arc<dyn ItemRepository>,
    vector_search: Option<Arc<dyn VectorSearch>>,
}

impl SearchEngine {
    pub fn new(item_repo: Arc<dyn ItemRepository>) -> Self {
        Self {
            item_repo,
            vector_search: None,
        }
    }

    pub fn with_vector_search(mut self, vs: Arc<dyn VectorSearch>) -> Self {
        self.vector_search = Some(vs);
        self
    }

    /// 执行搜索
    pub async fn search(&self, query: SearchQuery) -> InfraResult<SearchResult<Item>> {
        // 如果查询为空，返回最新的
        if query.text.trim().is_empty() {
            return self.get_recent(&query).await;
        }

        // 优先使用语义搜索
        if let Some(vs) = &self.vector_search {
            return self.semantic_search(vs, &query).await;
        }

        // 降级到关键词搜索
        self.keyword_search(&query).await
    }

    /// 关键词搜索
    async fn keyword_search(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        let has_filter = query.filter.item_type.is_some() || !query.filter.tags.is_empty();

        if !has_filter {
            let total = self.item_repo.count_text_hits(&query.text).await?;
            let items = self
                .item_repo
                .search_text(&query.text, query.pagination.offset, query.pagination.limit)
                .await?;
            let hits: Vec<SearchHit<Item>> = items
                .into_iter()
                .map(|item| SearchHit::new(item, 1.0))
                .collect();

            return Ok(SearchResult {
                items: hits,
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

        let hits: Vec<SearchHit<Item>> = page_items
            .into_iter()
            .map(|item| SearchHit::new(item, 1.0))
            .collect();

        Ok(SearchResult {
            items: hits,
            total,
            query: query.clone(),
        })
    }

    /// 语义搜索
    async fn semantic_search(
        &self,
        vs: &Arc<dyn VectorSearch>,
        query: &SearchQuery,
    ) -> InfraResult<SearchResult<Item>> {
        let request_limit = query
            .pagination
            .offset
            .saturating_add(query.pagination.limit)
            .saturating_add(100);

        // 获取相似文档 ID
        let similar = vs.search(&query.text, request_limit).await?;

        // 获取完整 Item
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

    /// 获取最近的 Item
    async fn get_recent(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        let all_items = if let Some(t) = &query.filter.item_type {
            self.item_repo.find_by_type(*t).await?
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
        let hits: Vec<SearchHit<Item>> = paginated_items
            .into_iter()
            .map(|item| SearchHit::new(item, 1.0))
            .collect();

        Ok(SearchResult {
            items: hits,
            total,
            query: query.clone(),
        })
    }

    /// 索引 Item
    pub async fn index_item(&self, item: &Item) -> InfraResult<()> {
        if let Some(vs) = &self.vector_search {
            let text = format!("{} {} {}", item.title(), item.summary(), item.content());
            vs.index(item.id().as_str(), &text).await?;
        }
        Ok(())
    }

    /// 从索引中删除
    pub async fn remove_from_index(&self, id: &str) -> InfraResult<()> {
        if let Some(vs) = &self.vector_search {
            vs.remove(id).await?;
        }
        Ok(())
    }

    fn matches_filter(item: &Item, filter: &SearchFilter) -> bool {
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
            .map(|t| t.as_str().to_lowercase())
            .collect();

        filter
            .tags
            .iter()
            .all(|tag| item_tags.contains(&tag.to_lowercase()))
    }

    fn paginate<T>(items: Vec<T>, offset: usize, limit: usize) -> Vec<T> {
        items.into_iter().skip(offset).take(limit).collect()
    }
}
