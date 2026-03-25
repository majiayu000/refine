//! 知识仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::RepoResult;
use crate::knowledge::{DocumentId, Item, ItemId, ItemType, Tag};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Item 仓储接口
#[async_trait]
pub trait ItemRepository: Send + Sync {
    /// 根据 ID 查找
    async fn find_by_id(&self, id: &ItemId) -> RepoResult<Option<Item>>;

    /// 查找所有
    async fn find_all(&self) -> RepoResult<Vec<Item>>;

    /// 按类型查找
    async fn find_by_type(&self, item_type: ItemType) -> RepoResult<Vec<Item>>;

    /// 按类型查找最近数据（分页）
    async fn find_recent(
        &self,
        item_type: Option<ItemType>,
        offset: usize,
        limit: usize,
    ) -> RepoResult<Vec<Item>>;

    /// 按类型统计总数
    async fn count_items(&self, item_type: Option<ItemType>) -> RepoResult<usize>;

    /// 按标签查找
    async fn find_by_tags(&self, tags: &[Tag]) -> RepoResult<Vec<Item>>;

    /// 保存（创建或更新）
    async fn save(&self, item: &Item) -> RepoResult<()>;

    /// 删除
    async fn delete(&self, id: &ItemId) -> RepoResult<bool>;

    /// 是否存在
    async fn exists(&self, id: &ItemId) -> RepoResult<bool>;

    /// 查找 since 时间点之后创建的所有 items
    async fn find_since(&self, since: DateTime<Utc>) -> RepoResult<Vec<Item>>;

    /// 查找 start..=end 时间范围内创建的 items（SQL: WHERE created_at BETWEEN start AND end）
    async fn find_by_date_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> RepoResult<Vec<Item>>;

    /// 全文搜索（分页）
    async fn search_text(&self, query: &str, offset: usize, limit: usize) -> RepoResult<Vec<Item>>;

    /// 全文搜索命中总数
    async fn count_text_hits(&self, query: &str) -> RepoResult<usize>;

    /// 按文档 ID 查找关联的 items
    async fn find_by_document_id(&self, doc_id: &DocumentId) -> RepoResult<Vec<Item>>;
}
