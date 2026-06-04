//! 知识仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::InfraResult;
use crate::knowledge::{DocumentId, Item, ItemId, ItemType, Tag};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Item 仓储接口
#[async_trait]
pub trait ItemRepository: Send + Sync {
    /// 根据 ID 查找
    async fn find_by_id(&self, id: &ItemId) -> InfraResult<Option<Item>>;

    /// 查找所有
    async fn find_all(&self) -> InfraResult<Vec<Item>>;

    /// 按类型查找
    async fn find_by_type(&self, item_type: ItemType) -> InfraResult<Vec<Item>>;

    /// 按类型查找最近数据（分页）
    async fn find_recent(
        &self,
        item_type: Option<ItemType>,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Item>>;

    /// 按类型统计总数
    async fn count_items(&self, item_type: Option<ItemType>) -> InfraResult<usize>;

    /// 按标签查找
    async fn find_by_tags(&self, tags: &[Tag]) -> InfraResult<Vec<Item>>;

    /// 保存（创建或更新）
    async fn save(&self, item: &Item) -> InfraResult<()>;

    /// 删除
    async fn delete(&self, id: &ItemId) -> InfraResult<bool>;

    /// 是否存在
    async fn exists(&self, id: &ItemId) -> InfraResult<bool>;

    /// 查找 since 时间点之后创建的所有 items
    async fn find_since(&self, since: DateTime<Utc>) -> InfraResult<Vec<Item>>;

    /// 查找 start..=end 时间范围内创建的 items（SQL: WHERE created_at BETWEEN start AND end）
    async fn find_by_date_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> InfraResult<Vec<Item>>;

    /// 查找事件时间窗口内的 observations。
    ///
    /// 会话导入的 item 使用关联 Document.captured_at；缺少关联 Document 的旧数据回退到 item.created_at。
    async fn find_observations_by_event_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> InfraResult<Vec<Item>>;

    /// 全文搜索（分页）
    async fn search_text(&self, query: &str, offset: usize, limit: usize)
        -> InfraResult<Vec<Item>>;

    /// 全文搜索命中总数
    async fn count_text_hits(&self, query: &str) -> InfraResult<usize>;

    /// 按文档 ID 查找关联的 items
    async fn find_by_document_id(&self, doc_id: &DocumentId) -> InfraResult<Vec<Item>>;
}
