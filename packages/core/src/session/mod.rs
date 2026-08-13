//! 会话分析模块
//!
//! 从 Claude Code / Codex 会话中提取认知观测

mod aggregation;
mod analysis_routes;
mod chunking;
mod clustering;
mod discovery;
mod facets;
mod filter;
mod parser;
mod prescription;
mod report;
mod types;

pub use aggregation::{aggregate_observations, format_report, AggregationReport};
pub use analysis_routes::{plan_routes, AnalysisRoute};
pub use chunking::{chunk_session, needs_chunking, SessionChunk};
pub use clustering::{
    cluster_observations, normalize_project_name, ClusterResult, GlobalStats, ProjectCluster,
};
pub use discovery::{discover_sessions, discover_sessions_in, DiscoveredSession};
pub use facets::{
    build_facet_prompt, facets_to_items, facets_to_items_with_mode, parse_facet_response,
    FacetResponse, FACET_SYSTEM_PROMPT,
};
pub use filter::{filter_sessions, passes_filter, FilterConfig};
pub use parser::{parse_session_content, parse_session_file};
pub use prescription::{build_prescription_prompt, PRESCRIPTION_SYSTEM_PROMPT};
pub use report::{
    build_final_prompt, merge_route_results, RouteResult, INSIGHTS_SYSTEM_PROMPT,
    ROUTE_SYSTEM_PROMPT,
};
pub use types::{MessageRole, Session, SessionMessage, SessionMeta, SessionMode, SessionSource};
