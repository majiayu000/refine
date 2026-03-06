//! 知识提炼模块 (核心域)
//!
//! ## 文件结构
//!
//! - `conversation.rs` - 对话解析
//! - `policy.rs` - 提炼策略
//! - `extractor.rs` - 提炼器
//! - `usecase.rs` - 提炼用例编排

mod conversation;
mod extractor;
mod policy;
mod usecase;

// 公共 API
pub use conversation::{Conversation, Message, Role};
pub use extractor::{ExtractionResult, Extractor};
pub use policy::{ExtractionPolicy, PromptTemplate};
pub use usecase::{
    apply_defaults, build_fallback_item, extract_items_or_fallback,
    extract_items_with_defaults, extract_items_with_llm, ItemExtractionInput,
    EXTRACTION_SYSTEM_PROMPT, JSON_REPAIR_SYSTEM_PROMPT,
};
