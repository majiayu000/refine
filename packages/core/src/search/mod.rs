//! 搜索模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `query.rs` - 查询对象
//! - `engine.rs` - 搜索引擎

mod engine;
mod query;

// 公共 API
pub use engine::{SearchEngine, VectorSearch};
pub use query::{Pagination, SearchFilter, SearchHit, SearchQuery, SearchResult, SearchScope};
