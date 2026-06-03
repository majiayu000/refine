//! Conversation 持久化领域
//!
//! 提供跨进程 / 跨入口可复用的 conversation / extraction-job / event 记录类型，
//! 以及 Repository trait。SQLite 实现位于 `crate::infra::sqlite`。
//!
//! 设计原则：
//! - record 是裸数据载体（fields 全 pub），不带业务行为
//! - HTTP / CLI DTO 由各 app 自己定义，不污染 core
//! - 所有 Repository trait 都是 `async` + `Send + Sync`，与 SqliteStore 模型一致

mod record;
mod repository;

pub use record::{
    normalize_timestamp, now_iso, ConversationRecord, ConversationStatus, EventRecord,
    ExtractionJobRecord, ExtractionMode, JobStatus,
};
pub use repository::{ConversationRepository, EventRepository, JobRepository};
