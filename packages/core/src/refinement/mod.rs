//! 知识提炼模块 (核心域)
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `conversation.rs` - 对话解析
//! - `policy.rs` - 提炼策略
//! - `extractor.rs` - 提炼器

mod conversation;
mod extractor;
mod policy;

// 公共 API
pub use conversation::{Conversation, Message, Role};
pub use extractor::{ExtractionResult, Extractor};
pub use policy::{ExtractionPolicy, PromptTemplate};
