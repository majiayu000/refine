//! 知识管理模块
//!
//! - `types.rs` - 值对象 (ItemId, DocumentId, Tag, Source, ItemType)
//! - `item.rs` - Item 聚合根
//! - `document.rs` - Document 聚合根
//! - `repository.rs` - Item 仓储接口
//! - `doc_repository.rs` - Document 仓储接口

mod doc_repository;
mod document;
mod item;
mod repository;
mod types;

pub use doc_repository::DocumentRepository;
pub use document::{Document, RestoreDocumentParams};
pub use item::{Item, RestoreParams};
pub use repository::{ItemRepository, ObservationDocumentMeta, ObservationWindowSnapshot};
pub use types::{DocumentId, ItemId, ItemType, Source, Tag};
