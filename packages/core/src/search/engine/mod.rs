//! 搜索引擎
//!
//! 组合关键词搜索和语义搜索

mod keyword;
mod semantic;

use crate::error::InfraResult;
use crate::knowledge::{Document, DocumentRepository, Item, ItemRepository};
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
    doc_repo: Option<Arc<dyn DocumentRepository>>,
    vector_search: Option<Arc<dyn VectorSearch>>,
}

impl SearchEngine {
    pub fn new(item_repo: Arc<dyn ItemRepository>) -> Self {
        Self {
            item_repo,
            doc_repo: None,
            vector_search: None,
        }
    }

    pub fn with_doc_repo(mut self, doc_repo: Arc<dyn DocumentRepository>) -> Self {
        self.doc_repo = Some(doc_repo);
        self
    }

    pub fn with_vector_search(mut self, vs: Arc<dyn VectorSearch>) -> Self {
        self.vector_search = Some(vs);
        self
    }

    /// 执行搜索（Items）
    pub async fn search(&self, query: SearchQuery) -> InfraResult<SearchResult<Item>> {
        if query.text.trim().is_empty() {
            return self.get_recent(&query).await;
        }

        if let Some(vs) = &self.vector_search {
            return self.hybrid_search(vs, &query).await;
        }

        self.keyword_search(&query).await
    }

    /// 执行文档搜索
    pub async fn search_documents(
        &self,
        query: &SearchQuery,
    ) -> InfraResult<SearchResult<Document>> {
        let Some(doc_repo) = &self.doc_repo else {
            return Ok(SearchResult::empty(query.clone()));
        };

        if query.text.trim().is_empty() {
            let total = doc_repo.count().await?;
            let docs = doc_repo
                .find_recent(query.pagination.offset, query.pagination.limit)
                .await?;
            return Ok(SearchResult {
                items: Self::to_doc_hits(docs),
                total,
                query: query.clone(),
            });
        }

        let total = doc_repo.count_text_hits(&query.text).await?;
        let docs = doc_repo
            .search_text(&query.text, query.pagination.offset, query.pagination.limit)
            .await?;
        Ok(SearchResult {
            items: Self::to_doc_hits(docs),
            total,
            query: query.clone(),
        })
    }

    /// 索引 Item
    pub async fn index_item(&self, item: &Item) -> InfraResult<()> {
        if let Some(vs) = &self.vector_search {
            let text = format!("{} {} {}", item.title(), item.summary(), item.content());
            vs.index(item.id().as_str(), &text)
                .await
                ?;
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

    fn to_hits(items: Vec<Item>) -> Vec<SearchHit<Item>> {
        items
            .into_iter()
            .map(|item| SearchHit::new(item, 1.0))
            .collect()
    }

    fn to_doc_hits(docs: Vec<Document>) -> Vec<SearchHit<Document>> {
        docs.into_iter()
            .map(|doc| SearchHit::new(doc, 1.0))
            .collect()
    }
}
