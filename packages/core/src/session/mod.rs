//! 会话分析模块
//!
//! 从 Claude Code / Codex 会话中提取认知观测

mod discovery;
mod parser;
mod types;

pub use discovery::{discover_sessions, discover_sessions_in, DiscoveredSession};
pub use parser::{parse_session_content, parse_session_file};
pub use types::{MessageRole, Session, SessionMessage, SessionMeta, SessionSource};
