use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::knowledge::{Item, ObservationDocumentMeta};
use refine_core::session::{
    eligible_observations, is_supported_session_document_source, AnalysisRoute, ClusterResult,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Read;

pub(crate) const MANIFEST_VERSION: u32 = 1;
pub(crate) const COHORT_CONTRACT_IDENTITY: &str = "source-aware-linked-interactive-v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EventTimeWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SourceStats {
    pub source: String,
    pub observation_count: usize,
    pub session_count: usize,
    pub freshest_event_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WindowManifest {
    pub event_time: EventTimeWindow,
    pub input_observations: usize,
    pub linked_observations: usize,
    pub detached_observations: usize,
    pub mode_excluded_observations: usize,
    pub source_excluded_observations: usize,
    pub eligible_observations: usize,
    pub linked_ratio: String,
    pub status: String,
    pub cohort_contract_identity: String,
    pub cohort_identity: String,
    pub source_counts: Vec<SourceStats>,
    pub unsupported_source_counts: Vec<SourceStats>,
    pub platform_unknown_observations: usize,
    pub platform_unknown_sessions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InsightsManifest {
    pub manifest_version: u32,
    pub mode: String,
    pub event_time_cutoff: DateTime<Utc>,
    pub current_window: WindowManifest,
    pub previous_window: Option<WindowManifest>,
    pub model_identity: String,
    pub prompt_identity: String,
    pub route_identity: String,
    pub binary_identity: String,
    pub source_revision: String,
}

#[derive(Default)]
struct SourceAccumulator {
    observation_count: usize,
    document_ids: HashSet<String>,
    freshest_event_time: Option<DateTime<Utc>>,
}

pub(crate) fn build_window_manifest(
    window: EventTimeWindow,
    cohort_observations: &[Item],
    all_observations: &[Item],
    cluster: &ClusterResult,
    documents: &[ObservationDocumentMeta],
) -> Result<WindowManifest> {
    let document_map: BTreeMap<&str, &ObservationDocumentMeta> = documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect();
    let eligible = eligible_observations(cohort_observations);
    let mut sources: BTreeMap<String, SourceAccumulator> = BTreeMap::new();
    for item in eligible {
        let document_id = item
            .document_id()
            .context("eligible observation unexpectedly lacks document_id")?;
        let document = document_map.get(document_id.as_str()).with_context(|| {
            format!(
                "eligible cohort references missing document {}",
                document_id
            )
        })?;
        let source = report_source(&document.source).to_string();
        let accumulator = sources.entry(source).or_default();
        accumulator.observation_count += 1;
        accumulator
            .document_ids
            .insert(document_id.as_str().to_string());
        accumulator.freshest_event_time = Some(
            accumulator
                .freshest_event_time
                .map_or(document.captured_at, |current| {
                    current.max(document.captured_at)
                }),
        );
    }

    let mut unsupported_sources: BTreeMap<String, SourceAccumulator> = BTreeMap::new();
    for item in all_observations {
        let Some(document_id) = item.document_id() else {
            continue;
        };
        let (source, captured_at) = document_map
            .get(document_id.as_str())
            .map(|document| (document.source.as_str(), Some(document.captured_at)))
            .unwrap_or(("missing_document_metadata", None));
        if is_supported_session_document_source(source) {
            continue;
        }
        let accumulator = unsupported_sources.entry(source.to_string()).or_default();
        accumulator.observation_count += 1;
        accumulator
            .document_ids
            .insert(document_id.as_str().to_string());
        if let Some(captured_at) = captured_at {
            accumulator.freshest_event_time = Some(
                accumulator
                    .freshest_event_time
                    .map_or(captured_at, |current| current.max(captured_at)),
            );
        }
    }

    let source_counts: Vec<SourceStats> = sources
        .into_iter()
        .map(|(source, accumulator)| SourceStats {
            source,
            observation_count: accumulator.observation_count,
            session_count: accumulator.document_ids.len(),
            freshest_event_time: accumulator.freshest_event_time,
        })
        .collect();
    let unsupported_source_counts = source_stats(unsupported_sources);
    let platform_unknown_observations = source_counts
        .iter()
        .filter(|stats| stats.source == "platform_unknown")
        .map(|stats| stats.observation_count)
        .sum();
    let platform_unknown_sessions = source_counts
        .iter()
        .filter(|stats| stats.source == "platform_unknown")
        .map(|stats| stats.session_count)
        .sum();
    let quality = &cluster.data_quality;

    Ok(WindowManifest {
        event_time: window,
        input_observations: quality.input_observations,
        linked_observations: quality.linked_observations,
        detached_observations: quality.detached_observations,
        mode_excluded_observations: quality.mode_excluded_observations,
        source_excluded_observations: quality.source_excluded_observations,
        eligible_observations: quality.eligible_observations,
        linked_ratio: format!("{:.6}", quality.linked_ratio()),
        status: quality.status_label().to_string(),
        cohort_contract_identity: COHORT_CONTRACT_IDENTITY.to_string(),
        cohort_identity: quality.cohort_identity.clone(),
        source_counts,
        unsupported_source_counts,
        platform_unknown_observations,
        platform_unknown_sessions,
    })
}

fn source_stats(sources: BTreeMap<String, SourceAccumulator>) -> Vec<SourceStats> {
    sources
        .into_iter()
        .map(|(source, accumulator)| SourceStats {
            source,
            observation_count: accumulator.observation_count,
            session_count: accumulator.document_ids.len(),
            freshest_event_time: accumulator.freshest_event_time,
        })
        .collect()
}

fn report_source(document_source: &str) -> &str {
    match document_source {
        "claude-code-session" => "claude",
        "codex-session" => "codex",
        // Remem currently preserves the archive container but not a reliable
        // upstream platform. It must remain unknown rather than being guessed.
        "remem-raw-session" => "platform_unknown",
        _ => "platform_unknown",
    }
}

pub(crate) fn build_manifest(
    mode: &str,
    cutoff: DateTime<Utc>,
    current_window: WindowManifest,
    previous_window: Option<WindowManifest>,
    model_identity: String,
    prompt_identity: &str,
    route_identity: String,
) -> InsightsManifest {
    InsightsManifest {
        manifest_version: MANIFEST_VERSION,
        mode: mode.to_string(),
        event_time_cutoff: cutoff,
        current_window,
        previous_window,
        model_identity,
        prompt_identity: prompt_identity.to_string(),
        route_identity,
        binary_identity: binary_identity(),
        source_revision: source_revision(),
    }
}

pub(crate) fn route_plan_identity(routes: &[AnalysisRoute]) -> String {
    let mut hasher = Sha256::new();
    for route in routes {
        hasher.update(route.id.to_le_bytes());
        hasher.update(route.title.as_bytes());
        hasher.update([0]);
        hasher.update(route.prompt.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn binary_identity() -> String {
    let Some(path) = std::env::current_exe().ok() else {
        return "unknown".to_string();
    };
    let Some(mut file) = std::fs::File::open(path).ok() else {
        return "unknown".to_string();
    };
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return "unknown".to_string(),
        }
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub(crate) fn source_revision() -> String {
    std::env::var("REFINE_SOURCE_REVISION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| option_env!("REFINE_SOURCE_REVISION").map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn manifest_identity(manifest: &InsightsManifest) -> Result<String> {
    let encoded = serde_json::to_vec(manifest).context("serialize insights manifest identity")?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

pub(crate) fn render_manifest(manifest: &InsightsManifest) -> Result<String> {
    let json = serde_json::to_string_pretty(manifest).context("serialize insights manifest")?;
    Ok(format!(
        "<!-- refine-insights-manifest-v1 -->\n```json\n{json}\n```"
    ))
}

pub(crate) fn rotation_seed(cohort_identity: &str) -> usize {
    cohort_identity.bytes().fold(0usize, |seed, byte| {
        seed.wrapping_mul(31).wrapping_add(byte as usize)
    })
}

pub(crate) fn build_delta_summary(
    current_cluster: &ClusterResult,
    current: &WindowManifest,
    previous_cluster: Option<&ClusterResult>,
    previous: Option<&WindowManifest>,
) -> String {
    let (Some(previous_cluster), Some(previous)) = (previous_cluster, previous) else {
        return "新增/消失/反转: 不适用于全历史 snapshot。\n证据缺口: 未提供前一等长窗口。"
            .to_string();
    };
    if current.status != "OK" || previous.status != "OK" {
        return format!(
            "新增/消失/反转: 已抑制；两个窗口中至少一个为 DEGRADED，禁止跨期趋势。\n证据缺口: current={} previous={}；current detached={} previous detached={}；current source-excluded={} previous source-excluded={}；platform unknown sessions current={} previous={}",
            current.status,
            previous.status,
            current.detached_observations,
            previous.detached_observations,
            current.source_excluded_observations,
            previous.source_excluded_observations,
            current.platform_unknown_sessions,
            previous.platform_unknown_sessions,
        );
    }

    let current_projects: BTreeSet<&str> = current_cluster
        .global_stats
        .project_ranking
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let previous_projects: BTreeSet<&str> = previous_cluster
        .global_stats
        .project_ranking
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    let additions = current_projects
        .difference(&previous_projects)
        .copied()
        .collect::<Vec<_>>();
    let removals = previous_projects
        .difference(&current_projects)
        .copied()
        .collect::<Vec<_>>();
    format!(
        "新增项目: {}。\n消失项目: {}。\n反转: 两窗口不足以判定方向反转，需要至少三个等长窗口。\n证据缺口: platform unknown sessions current={} previous={}。\n可比总量: sessions {}→{}；decisions {}→{}；bugfixes {}→{}。",
        list_or_none(&additions),
        list_or_none(&removals),
        current.platform_unknown_sessions,
        previous.platform_unknown_sessions,
        previous_cluster.global_stats.total_sessions,
        current_cluster.global_stats.total_sessions,
        previous_cluster.global_stats.total_decisions,
        current_cluster.global_stats.total_decisions,
        previous_cluster.global_stats.total_bugfixes,
        current_cluster.global_stats.total_bugfixes,
    )
}

fn list_or_none(values: &[&str]) -> String {
    if values.is_empty() {
        "无".to_string()
    } else {
        values.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::knowledge::DocumentId;
    use refine_core::session::{
        cluster_session_observations, ClusterResult, DataQualityStats, GlobalStats,
    };
    use std::collections::HashMap;

    fn cluster(projects: &[(&str, usize)], degraded: bool) -> ClusterResult {
        ClusterResult {
            projects: HashMap::new(),
            global_stats: GlobalStats {
                total_sessions: projects.iter().map(|(_, count)| count).sum(),
                total_decisions: 3,
                total_bugfixes: 2,
                total_summaries: 4,
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tool_frequency: HashMap::new(),
                project_ranking: projects
                    .iter()
                    .map(|(name, count)| ((*name).to_string(), *count))
                    .collect(),
            },
            data_quality: DataQualityStats {
                detached_observations: usize::from(degraded),
                ..DataQualityStats::default()
            },
            untagged_count: 0,
        }
    }

    fn window(status: &str) -> WindowManifest {
        let now = Utc::now();
        WindowManifest {
            event_time: EventTimeWindow {
                start: Some(now),
                end: Some(now),
            },
            input_observations: 1,
            linked_observations: 1,
            detached_observations: usize::from(status == "DEGRADED"),
            mode_excluded_observations: 0,
            source_excluded_observations: 0,
            eligible_observations: 1,
            linked_ratio: "1.000000".into(),
            status: status.into(),
            cohort_contract_identity: COHORT_CONTRACT_IDENTITY.into(),
            cohort_identity: "sha256:test".into(),
            source_counts: Vec::new(),
            unsupported_source_counts: Vec::new(),
            platform_unknown_observations: 0,
            platform_unknown_sessions: 0,
        }
    }

    #[test]
    fn delta_lists_additions_and_removals_before_stable_counts() {
        let current_cluster = cluster(&[("new", 2), ("steady", 1)], false);
        let previous_cluster = cluster(&[("old", 2), ("steady", 1)], false);
        let delta = build_delta_summary(
            &current_cluster,
            &window("OK"),
            Some(&previous_cluster),
            Some(&window("OK")),
        );
        assert!(delta.contains("新增项目: new"));
        assert!(delta.contains("消失项目: old"));
        assert!(delta.find("新增项目").unwrap() < delta.find("可比总量").unwrap());
    }

    #[test]
    fn previous_only_window_reports_disappeared_projects() {
        let current_cluster = cluster(&[], false);
        let previous_cluster = cluster(&[("inactive", 2)], false);
        let delta = build_delta_summary(
            &current_cluster,
            &window("OK"),
            Some(&previous_cluster),
            Some(&window("OK")),
        );
        assert!(delta.contains("消失项目: inactive"));
        assert!(delta.contains("sessions 2→0"));
    }

    #[test]
    fn unsupported_document_sources_are_excluded_and_visible() {
        let now = Utc::now();
        let mut codex = Item::new_observation("codex", "codex evidence");
        codex.set_document_id(DocumentId::from("codex-doc"));
        let mut grok = Item::new_observation("grok", "legacy knowledge evidence");
        grok.set_document_id(DocumentId::from("grok-doc"));
        let observations = vec![codex, grok];
        let sources = HashMap::from([
            ("codex-doc".into(), "codex-session".into()),
            ("grok-doc".into(), "grok-knowledge".into()),
        ]);
        let cohort = cluster_session_observations(&observations, &sources);
        let documents = vec![
            ObservationDocumentMeta {
                id: DocumentId::from("codex-doc"),
                source: "codex-session".into(),
                captured_at: now,
            },
            ObservationDocumentMeta {
                id: DocumentId::from("grok-doc"),
                source: "grok-knowledge".into(),
                captured_at: now,
            },
        ];

        let manifest = build_window_manifest(
            EventTimeWindow {
                start: Some(now),
                end: Some(now),
            },
            &cohort.cohort_items,
            &observations,
            &cohort.cluster,
            &documents,
        )
        .unwrap();

        assert_eq!(manifest.eligible_observations, 1);
        assert_eq!(manifest.source_excluded_observations, 1);
        assert_eq!(manifest.status, "DEGRADED");
        assert_eq!(manifest.source_counts[0].source, "codex");
        assert_eq!(
            manifest.unsupported_source_counts[0].source,
            "grok-knowledge"
        );
        assert_eq!(manifest.unsupported_source_counts[0].observation_count, 1);
    }

    #[test]
    fn degraded_window_suppresses_all_trend_numbers() {
        let current_cluster = cluster(&[("new", 2)], true);
        let previous_cluster = cluster(&[("old", 2)], false);
        let delta = build_delta_summary(
            &current_cluster,
            &window("DEGRADED"),
            Some(&previous_cluster),
            Some(&window("OK")),
        );
        assert!(delta.contains("已抑制"));
        assert!(!delta.contains("sessions 2→2"));
    }

    #[test]
    fn remem_is_platform_unknown_not_claude_or_codex() {
        assert_eq!(report_source("remem-raw-session"), "platform_unknown");
        assert_eq!(report_source("future-session-source"), "platform_unknown");
        assert_eq!(report_source("claude-code-session"), "claude");
        assert_eq!(report_source("codex-session"), "codex");
    }

    #[test]
    fn rendered_manifest_is_visible_json_with_required_reproducibility_fields() {
        let now = Utc::now();
        let manifest = InsightsManifest {
            manifest_version: MANIFEST_VERSION,
            mode: "rolling-7d-delta".into(),
            event_time_cutoff: now,
            current_window: window("OK"),
            previous_window: Some(window("OK")),
            model_identity: "provider:model:endpoint-sha256:test".into(),
            prompt_identity: "prompt-v1".into(),
            route_identity: "sha256:route".into(),
            binary_identity: "refine-cli/test".into(),
            source_revision: "unknown".into(),
        };
        let rendered = render_manifest(&manifest).unwrap();
        assert!(rendered.starts_with("<!-- refine-insights-manifest-v1 -->\n```json"));
        for field in [
            "event_time_cutoff",
            "current_window",
            "previous_window",
            "source_counts",
            "model_identity",
            "prompt_identity",
            "route_identity",
            "binary_identity",
            "source_revision",
        ] {
            assert!(rendered.contains(&format!("\"{field}\"")));
        }
    }
}
