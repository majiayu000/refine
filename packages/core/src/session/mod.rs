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
mod project_identity;
mod project_identity_value;
mod remem_archive;
mod report;
mod source_cohort;
mod types;

pub use aggregation::{aggregate_observations, format_report, AggregationReport};
pub use analysis_routes::{plan_routes, AnalysisRoute};
pub use chunking::{chunk_session, needs_chunking, SessionChunk};
pub use clustering::{
    cluster_observation_windows, cluster_observations, cluster_observations_with_resolver,
    eligible_observations, ClusterResult, DataQualityStats, GlobalStats, ProjectCluster,
};
pub use discovery::{discover_sessions, discover_sessions_in, DiscoveredSession};
pub use facets::{
    build_facet_prompt, facets_to_items, facets_to_items_with_mode,
    facets_to_items_with_mode_and_identity, parse_facet_response, FacetResponse,
    FACET_SYSTEM_PROMPT,
};
pub use filter::{
    filter_sessions, is_looper_scheduled_skill_first_user_message,
    is_looper_scheduled_skill_session, passes_filter, FilterConfig,
};
pub use parser::{parse_session_content, parse_session_file};
pub use prescription::{build_prescription_prompt, PRESCRIPTION_SYSTEM_PROMPT};
pub use project_identity::ProjectIdentityResolver;
pub use remem_archive::{
    is_missing_remem_executable, load_remem_document_content, load_remem_session,
    load_remem_session_summaries, remem_snapshot_hash, RememSession, RememSessionSummary,
};
pub use report::{
    build_final_prompt, build_final_prompt_with_delta, build_final_prompt_with_delta_and_budget,
    format_data_quality_stats, merge_route_results, merge_route_results_with_budget, RouteResult,
    INSIGHTS_SYSTEM_PROMPT, ROUTE_SYSTEM_PROMPT,
};
pub use source_cohort::{
    cluster_session_observation_windows, cluster_session_observations,
    is_supported_session_document_source, portrait_session_observation_windows,
    portrait_session_observations, PortraitGlobalStats, PortraitSessionCohort,
    SessionCohortCluster, SUPPORTED_SESSION_DOCUMENT_SOURCES,
};
pub use types::{MessageRole, Session, SessionMessage, SessionMeta, SessionMode, SessionSource};
