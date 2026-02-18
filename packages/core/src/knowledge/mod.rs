//! 知识管理模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `types.rs` - 值对象 (ItemId, Tag, Source, ItemType)
//! - `item.rs` - Item 聚合根
//! - `repository.rs` - 仓储接口

mod item;
mod repository;
mod types;

// 公共 API
pub use item::{Item, RestoreParams};
pub use repository::ItemRepository;
pub use types::{ItemId, ItemType, Source, Tag};
