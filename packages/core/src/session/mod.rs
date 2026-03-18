//! 会话分析模块
//!
//! 从 Claude Code / Codex 会话中提取认知观测

mod aggregation;
mod chunking;
mod discovery;
mod facets;
mod filter;
mod parser;
mod types;

pub use aggregation::{aggregate_observations, format_report, AggregationReport};
pub use chunking::{chunk_session, needs_chunking, SessionChunk};
pub use discovery::{discover_sessions, discover_sessions_in, DiscoveredSession};
pub use facets::{
    build_facet_prompt, facets_to_items, parse_facet_response, FacetResponse,
    FACET_SYSTEM_PROMPT,
};
pub use filter::{filter_sessions, passes_filter, FilterConfig};
pub use parser::{parse_session_content, parse_session_file};
pub use types::{MessageRole, Session, SessionMessage, SessionMeta, SessionSource};
