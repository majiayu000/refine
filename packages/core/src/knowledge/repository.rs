//! 知识仓储接口
//!
//! 定义在领域层，实现在 infra 层

use crate::error::InfraResult;
use crate::knowledge::{Item, ItemId, ItemType, Tag};
use async_trait::async_trait;

/// Item 仓储接口
#[async_trait]
pub trait ItemRepository: Send + Sync {
    /// 根据 ID 查找
    async fn find_by_id(&self, id: &ItemId) -> InfraResult<Option<Item>>;

    /// 查找所有
    async fn find_all(&self) -> InfraResult<Vec<Item>>;

    /// 按类型查找
    async fn find_by_type(&self, item_type: ItemType) -> InfraResult<Vec<Item>>;

    /// 按标签查找
    async fn find_by_tags(&self, tags: &[Tag]) -> InfraResult<Vec<Item>>;

    /// 保存（创建或更新）
    async fn save(&self, item: &Item) -> InfraResult<()>;

    /// 删除
    async fn delete(&self, id: &ItemId) -> InfraResult<bool>;

    /// 是否存在
    async fn exists(&self, id: &ItemId) -> InfraResult<bool>;

    /// 全文搜索
    async fn search_text(&self, query: &str, limit: usize) -> InfraResult<Vec<Item>>;
}
