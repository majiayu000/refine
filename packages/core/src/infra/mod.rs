//! 基础设施模块
//!
//! ## 文件结构
//!
//! - `sqlite/` - SQLite 存储实现（worker + 查询）
//! - `llm.rs` - LLM 客户端 (Claude, OpenAI)
//! - `schema.sql` - 数据库 Schema

mod contract;
mod llm;
mod sqlite;

// 公共 API
pub use contract::{
    is_contract_compatible, normalize_contract_major, CONTRACT_VERSION, CONTRACT_VERSION_HEADER,
};
pub use llm::{
    build_llm_client_from_env, build_required_llm_client_from_env, ClaudeClient, LlmClient,
    OpenAIClient,
};
pub use sqlite::SqliteStore;
