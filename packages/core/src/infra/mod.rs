//! 基础设施模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `sqlite.rs` - SQLite 存储实现
//! - `llm.rs` - LLM 客户端 (Claude, OpenAI)
//! - `schema.sql` - 数据库 Schema

mod llm;
mod sqlite;

// 公共 API
pub use llm::{ClaudeClient, LlmClient, OpenAIClient};
pub use sqlite::SqliteStore;
