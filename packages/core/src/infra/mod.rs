//! 基础设施模块
//!
//! ## 文件结构
//!
//! - `sqlite/` - SQLite 存储实现（worker + 查询）
//! - `llm.rs` - LLM 客户端 (Claude, OpenAI)
//! - `schema.sql` - 数据库 Schema

mod contract;
mod llm;
mod paths;
mod sqlite;

// 公共 API
pub use contract::{
    contract_incompatible_message, is_contract_compatible, normalize_contract_major,
    normalize_conversation_input, trim_optional, trim_required_field, validate_contract_version,
    CreateConversationRequest, DocumentDetailDto, DocumentDto, ItemDto,
    NormalizedConversationInput, CONTRACT_VERSION, CONTRACT_VERSION_HEADER,
};
pub use llm::{
    build_llm_client_from_env, build_required_llm_client_from_env, ClaudeClient, LlmClient,
    OpenAIClient,
};
pub use paths::{default_db_path, ensure_db_dir, resolve_db_path};
pub use sqlite::SqliteStore;
