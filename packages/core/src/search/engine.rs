//! 搜索引擎
//!
//! 组合关键词搜索和语义搜索

use crate::error::InfraResult;
use crate::knowledge::{Item, ItemRepository};
use crate::search::query::{SearchHit, SearchQuery, SearchResult};
use async_trait::async_trait;
use std::sync::Arc;

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
        if query.text.is_empty() {
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
        let items = self
            .item_repo
            .search_text(&query.text, query.pagination.limit)
            .await?;

        let hits: Vec<SearchHit<Item>> = items
            .into_iter()
            .map(|item| SearchHit::new(item, 1.0))
            .collect();

        let total = hits.len();

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
        // 获取相似文档 ID
        let similar = vs.search(&query.text, query.pagination.limit).await?;

        // 获取完整 Item
        let mut hits = Vec::new();
        for (id, score) in similar {
            if let Some(item) = self
                .item_repo
                .find_by_id(&crate::knowledge::ItemId::from_str(&id))
                .await?
            {
                // 应用过滤
                if let Some(t) = &query.filter.item_type {
                    if item.item_type() != *t {
                        continue;
                    }
                }
                hits.push(SearchHit::new(item, score));
            }
        }

        let total = hits.len();

        Ok(SearchResult {
            items: hits,
            total,
            query: query.clone(),
        })
    }

    /// 获取最近的 Item
    async fn get_recent(&self, query: &SearchQuery) -> InfraResult<SearchResult<Item>> {
        let items = if let Some(t) = &query.filter.item_type {
            self.item_repo.find_by_type(*t).await?
        } else {
            self.item_repo.find_all().await?
        };

        let hits: Vec<SearchHit<Item>> = items
            .into_iter()
            .take(query.pagination.limit)
            .map(|item| SearchHit::new(item, 1.0))
            .collect();

        let total = hits.len();

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
}
