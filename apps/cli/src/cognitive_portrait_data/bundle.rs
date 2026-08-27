use crate::insights_manifest::{
    build_manifest, build_window_manifest, report_source, EventTimeWindow, InsightsManifest,
    WindowManifest,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::knowledge::{
    Item, ItemRepository, ObservationDocumentMeta, ObservationWindowSnapshot,
};
use refine_core::session::{cluster_session_observations, eligible_observations, ClusterResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

pub(crate) const PORTRAIT_BUNDLE_SCHEMA_VERSION: u32 = 1;
pub(crate) const PORTRAIT_COLLECTOR_VERSION: &str = "cognitive-portrait-collector-v1";
const PORTRAIT_PROMPT_IDENTITY: &str = "cognitive-portrait-v4:evidence-bundle-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CognitivePortraitBundle {
    pub schema_version: u32,
    pub collector_version: String,
    pub period_days: usize,
    pub cutoff: DateTime<Utc>,
    pub manifest: InsightsManifest,
    pub comparison: ComparisonContract,
    pub current: PortraitWindowData,
    pub previous: PortraitWindowData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComparisonContract {
    pub comparable: bool,
    pub status: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitWindowData {
    pub metrics: PortraitMetrics,
    pub evidence: Vec<EvidenceRecord>,
    pub dimensions: PortraitDimensions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitMetrics {
    pub total_sessions: usize,
    pub total_decisions: usize,
    pub total_bugfixes: usize,
    pub total_summaries: usize,
    pub untagged_observations: usize,
    pub project_ranking: Vec<(String, usize)>,
    pub cognitive_levels: BTreeMap<String, usize>,
    pub collaboration_modes: BTreeMap<String, usize>,
    pub tool_frequency: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitDimensions {
    pub projects: Vec<DimensionEvidence>,
    pub decisions: Vec<DimensionEvidence>,
    pub bugfixes: Vec<DimensionEvidence>,
    pub knowledge: Vec<DimensionEvidence>,
    pub patterns: Vec<DimensionEvidence>,
    pub architectures: Vec<DimensionEvidence>,
    pub frictions: Vec<DimensionEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DimensionEvidence {
    pub value: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EvidenceRecord {
    pub evidence_id: String,
    pub item_id: String,
    pub document_id: String,
    pub event_time: DateTime<Utc>,
    pub source: String,
    pub project: String,
    pub categories: Vec<String>,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub excerpt: Option<String>,
    pub tags: Vec<String>,
}

pub(crate) async fn collect_bundle(
    item_store: &dyn ItemRepository,
    cutoff: DateTime<Utc>,
    period_days: usize,
) -> Result<CognitivePortraitBundle> {
    if period_days == 0 {
        bail!("INVALID_PERIOD: cognitive portrait period must be greater than zero");
    }
    let snapshot = item_store
        .load_observation_window_snapshot(cutoff, Some(period_days))
        .await
        .context(
            "SCHEMA_INVALID: failed to load the cognitive portrait windows from the real schema",
        )?;
    build_bundle_from_snapshot(snapshot, cutoff, period_days)
}

pub(crate) fn build_bundle_from_snapshot(
    snapshot: ObservationWindowSnapshot,
    cutoff: DateTime<Utc>,
    period_days: usize,
) -> Result<CognitivePortraitBundle> {
    if period_days == 0 {
        bail!("INVALID_PERIOD: cognitive portrait period must be greater than zero");
    }
    if snapshot.current.is_empty() && snapshot.previous.is_empty() {
        bail!("NO_CORE_DATA: current and previous windows contain no Observation rows");
    }
    let days = i64::try_from(period_days).context("portrait period exceeds i64")?;
    let current_start = cutoff
        .checked_sub_signed(Duration::days(days))
        .context("portrait current window underflow")?;
    let previous_start = current_start
        .checked_sub_signed(Duration::days(days))
        .context("portrait previous window underflow")?;
    let document_sources: HashMap<String, String> = snapshot
        .documents
        .iter()
        .map(|document| (document.id.as_str().to_string(), document.source.clone()))
        .collect();
    let current_cohort = cluster_session_observations(&snapshot.current, &document_sources);
    let previous_cohort = cluster_session_observations(&snapshot.previous, &document_sources);
    if current_cohort.cluster.data_quality.eligible_observations == 0 {
        bail!(
            "NO_CORE_DATA: current rolling window contains no eligible linked session observations"
        );
    }

    let current_manifest = build_window_manifest(
        EventTimeWindow {
            start: Some(current_start),
            end: Some(cutoff),
        },
        &current_cohort.cohort_items,
        &snapshot.current,
        &current_cohort.cluster,
        &snapshot.documents,
    )?;
    let previous_manifest = build_window_manifest(
        EventTimeWindow {
            start: Some(previous_start),
            end: Some(current_start),
        },
        &previous_cohort.cohort_items,
        &snapshot.previous,
        &previous_cohort.cluster,
        &snapshot.documents,
    )?;
    let comparison = comparison_contract(&current_manifest, &previous_manifest);
    let manifest = build_manifest(
        &format!("rolling-{period_days}d-cognitive-portrait"),
        cutoff,
        current_manifest,
        Some(previous_manifest),
        "none:deterministic-collector".to_string(),
        PORTRAIT_PROMPT_IDENTITY,
        collector_identity(),
    );
    let current = build_window_data(
        &current_cohort.cohort_items,
        &current_cohort.cluster,
        &snapshot.documents,
    )?;
    let previous = build_window_data(
        &previous_cohort.cohort_items,
        &previous_cohort.cluster,
        &snapshot.documents,
    )?;
    Ok(CognitivePortraitBundle {
        schema_version: PORTRAIT_BUNDLE_SCHEMA_VERSION,
        collector_version: PORTRAIT_COLLECTOR_VERSION.to_string(),
        period_days,
        cutoff,
        manifest,
        comparison,
        current,
        previous,
    })
}

fn comparison_contract(current: &WindowManifest, previous: &WindowManifest) -> ComparisonContract {
    let mut reasons = Vec::new();
    if current.status != "OK" {
        reasons.push(format!("current_window_status={}", current.status));
    }
    if previous.status != "OK" {
        reasons.push(format!("previous_window_status={}", previous.status));
    }
    if previous.eligible_observations == 0 {
        reasons.push("previous_window_has_no_eligible_observations".to_string());
    }
    if current.cohort_contract_identity != previous.cohort_contract_identity {
        reasons.push("cohort_contract_identity_mismatch".to_string());
    }
    let comparable = reasons.is_empty();
    ComparisonContract {
        comparable,
        status: if comparable { "OK" } else { "DEGRADED" }.to_string(),
        reasons,
    }
}

fn collector_identity() -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(PORTRAIT_COLLECTOR_VERSION.as_bytes())
    )
}

fn build_window_data(
    cohort_items: &[Item],
    cluster: &ClusterResult,
    documents: &[ObservationDocumentMeta],
) -> Result<PortraitWindowData> {
    let documents: BTreeMap<&str, &ObservationDocumentMeta> = documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect();
    let eligible = eligible_observations(cohort_items);
    let mut dimensions = DimensionAccumulator::default();
    let mut evidence = Vec::with_capacity(eligible.len());
    for item in eligible {
        let document_id = item
            .document_id()
            .context("eligible portrait observation unexpectedly lacks document_id")?;
        let document = documents.get(document_id.as_str()).with_context(|| {
            format!(
                "eligible portrait observation references missing document metadata {}",
                document_id
            )
        })?;
        let evidence_id = format!("obs:{}", item.id());
        let tags: Vec<String> = item
            .tags()
            .iter()
            .map(|tag| tag.as_str().to_string())
            .collect();
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let project = project_for_item(cluster, item);
        let mut categories = BTreeSet::new();
        dimensions.projects.add(&project, &evidence_id);
        if tag_refs.contains(&"decision") {
            categories.insert("decision".to_string());
            dimensions.decisions.add(item.title(), &evidence_id);
        } else if tag_refs.contains(&"bugfix") {
            categories.insert("bugfix".to_string());
            dimensions.bugfixes.add(item.title(), &evidence_id);
        } else {
            categories.insert("summary".to_string());
        }
        add_sections(item, &evidence_id, &mut categories, &mut dimensions);
        evidence.push(EvidenceRecord {
            evidence_id,
            item_id: item.id().as_str().to_string(),
            document_id: document_id.as_str().to_string(),
            event_time: document.captured_at,
            source: report_source(&document.source).to_string(),
            project,
            categories: categories.into_iter().collect(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            excerpt: item.excerpt().map(ToOwned::to_owned),
            tags,
        });
    }
    evidence.sort_by(|left, right| {
        left.event_time
            .cmp(&right.event_time)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    let stats = &cluster.global_stats;
    Ok(PortraitWindowData {
        metrics: PortraitMetrics {
            total_sessions: stats.total_sessions,
            total_decisions: stats.total_decisions,
            total_bugfixes: stats.total_bugfixes,
            total_summaries: stats.total_summaries,
            untagged_observations: cluster.untagged_count,
            project_ranking: stats.project_ranking.clone(),
            cognitive_levels: stats.cognitive_levels.clone().into_iter().collect(),
            collaboration_modes: stats.collaboration_modes.clone().into_iter().collect(),
            tool_frequency: stats.tool_frequency.clone().into_iter().collect(),
        },
        evidence,
        dimensions: dimensions.finish(),
    })
}

fn add_sections(
    item: &Item,
    evidence_id: &str,
    categories: &mut BTreeSet<String>,
    dimensions: &mut DimensionAccumulator,
) {
    for (category, section, target) in [
        ("knowledge", "知识", &mut dimensions.knowledge),
        ("pattern", "模式", &mut dimensions.patterns),
        ("architecture", "架构", &mut dimensions.architectures),
        ("friction", "阻力", &mut dimensions.frictions),
    ] {
        for value in extract_section_items(item.content(), section) {
            categories.insert(category.to_string());
            target.add(&value, evidence_id);
        }
    }
}

fn project_for_item(cluster: &ClusterResult, item: &Item) -> String {
    let summary_prefix = format!("【{}】", item.title());
    let mut project_names: Vec<&str> = cluster.projects.keys().map(String::as_str).collect();
    project_names.sort_unstable();
    project_names
        .into_iter()
        .find(|project_name| {
            let project = &cluster.projects[*project_name];
            project
                .decision_titles
                .iter()
                .chain(project.bugfix_titles.iter())
                .any(|title| title == item.title())
                || project
                    .summary_excerpts
                    .iter()
                    .any(|summary| summary.starts_with(&summary_prefix))
        })
        .unwrap_or("other")
        .to_string()
}

fn extract_section_items(content: &str, section: &str) -> Vec<String> {
    let mut in_section = false;
    let mut values = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.starts_with('-') {
            in_section = trimmed.trim_end_matches(':') == section;
            continue;
        }
        if in_section && trimmed.starts_with("- ") {
            values.push(trimmed.trim_start_matches("- ").to_string());
        } else if in_section && !trimmed.is_empty() {
            in_section = false;
        }
    }
    values
}

#[derive(Default)]
struct DimensionAccumulator {
    projects: EvidenceValues,
    decisions: EvidenceValues,
    bugfixes: EvidenceValues,
    knowledge: EvidenceValues,
    patterns: EvidenceValues,
    architectures: EvidenceValues,
    frictions: EvidenceValues,
}

impl DimensionAccumulator {
    fn finish(self) -> PortraitDimensions {
        PortraitDimensions {
            projects: self.projects.finish(),
            decisions: self.decisions.finish(),
            bugfixes: self.bugfixes.finish(),
            knowledge: self.knowledge.finish(),
            patterns: self.patterns.finish(),
            architectures: self.architectures.finish(),
            frictions: self.frictions.finish(),
        }
    }
}

#[derive(Default)]
struct EvidenceValues(BTreeMap<String, BTreeSet<String>>);

impl EvidenceValues {
    fn add(&mut self, value: &str, evidence_id: &str) {
        self.0
            .entry(value.to_string())
            .or_default()
            .insert(evidence_id.to_string());
    }

    fn finish(self) -> Vec<DimensionEvidence> {
        self.0
            .into_iter()
            .map(|(value, evidence_ids)| DimensionEvidence {
                value,
                evidence_ids: evidence_ids.into_iter().collect(),
            })
            .collect()
    }
}

pub(crate) fn write_bundle(path: &Path, bundle: &CognitivePortraitBundle) -> Result<()> {
    let mut json = serde_json::to_string_pretty(bundle).context("serialize portrait bundle")?;
    json.push('\n');
    fs::write(path, json)
        .with_context(|| format!("write cognitive portrait bundle {}", path.display()))
}

pub(crate) fn read_bundle(path: &Path) -> Result<CognitivePortraitBundle> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read cognitive portrait bundle {}", path.display()))?;
    let bundle: CognitivePortraitBundle = serde_json::from_str(&raw)
        .with_context(|| format!("SCHEMA_INVALID: parse portrait bundle {}", path.display()))?;
    if bundle.schema_version != PORTRAIT_BUNDLE_SCHEMA_VERSION {
        bail!(
            "SCHEMA_INVALID: unsupported portrait bundle schema {}; expected {}",
            bundle.schema_version,
            PORTRAIT_BUNDLE_SCHEMA_VERSION
        );
    }
    if bundle.collector_version != PORTRAIT_COLLECTOR_VERSION {
        bail!(
            "SCHEMA_INVALID: unsupported collector version {}; expected {}",
            bundle.collector_version,
            PORTRAIT_COLLECTOR_VERSION
        );
    }
    Ok(bundle)
}
