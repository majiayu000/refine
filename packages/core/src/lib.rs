//! Refine Core Library
//!
//! 智能知识复用引擎的核心库
//!
//! ## 模块结构（固定，禁止新增顶层模块）
//!
//! - `error` - 统一错误类型
//! - `knowledge` - 知识管理（Item, Tag, Source）
//! - `refinement` - 知识提炼（Conversation, Extractor）
//! - `search` - 搜索引擎（Query, Engine）
//! - `infra` - 基础设施（SQLite, LLM）
//!
//! ## 使用示例
//!
//! ```ignore
//! use refine_core::{
//!     knowledge::{Item, ItemType},
//!     refinement::{Conversation, Extractor},
//!     search::{SearchEngine, SearchQuery},
//!     infra::{SqliteStore, LiteLlmClient},
//! };
//!
//! // 创建存储
//! let store = SqliteStore::open("refine.db")?;
//!
//! // 解析对话
//! let conv = Conversation::parse("Human: 如何写 Rust?\nAssistant: ...")?;
//!
//! // 提炼知识
//! let extractor = Extractor::with_default_policy();
//! let result = extractor.parse_response(&llm_response, &conv)?;
//!
//! // 保存 Item
//! for item in result.items {
//!     store.save(&item).await?;
//! }
//!
//! // 搜索
//! let engine = SearchEngine::new(Arc::new(store));
//! let results = engine.search(SearchQuery::new("Rust")).await?;
//! ```

// 禁止新增顶层模块
pub mod error;
pub mod infra;
pub mod knowledge;
pub mod refinement;
pub mod search;

// 常用类型重导出
pub use error::{AppError, DomainError, InfraError, Result};
