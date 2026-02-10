//! 搜索引擎
//!
//! 组合关键词搜索和语义搜索

mod keyword;
mod semantic;

use crate::error::{InfraResult, RepoResult, RepositoryError};
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
    pub async fn search(&self, query: SearchQuery) -> RepoResult<SearchResult<Item>> {
        if query.text.trim().is_empty() {
            return self.get_recent(&query).await;
        }

        if let Some(vs) = &self.vector_search {
            return self.hybrid_search(vs, &query).await;
        }

        self.keyword_search(&query).await
    }

    /// 索引 Item
    pub async fn index_item(&self, item: &Item) -> RepoResult<()> {
        if let Some(vs) = &self.vector_search {
            let text = format!("{} {} {}", item.title(), item.summary(), item.content());
            vs.index(item.id().as_str(), &text)
                .await
                .map_err(RepositoryError::from)?;
        }
        Ok(())
    }

    /// 从索引中删除
    pub async fn remove_from_index(&self, id: &str) -> RepoResult<()> {
        if let Some(vs) = &self.vector_search {
            vs.remove(id).await.map_err(RepositoryError::from)?;
        }
        Ok(())
    }

    fn to_hits(items: Vec<Item>) -> Vec<SearchHit<Item>> {
        items
            .into_iter()
            .map(|item| SearchHit::new(item, 1.0))
            .collect()
    }
}
