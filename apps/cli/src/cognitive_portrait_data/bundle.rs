use crate::insights_manifest::{
    build_manifest, build_window_manifest_from_refs, validate_window_manifest, EventTimeWindow,
    InsightsManifest, WindowManifest,
};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::knowledge::{ItemRepository, ObservationWindowSnapshot};
use refine_core::session::portrait_session_observations;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use super::projection::{build_window_data, enforce_bundle_budgets, validate_window_projection};
use super::{read_utf8_bounded, MAX_PORTRAIT_BUNDLE_BYTES};

pub(crate) const PORTRAIT_BUNDLE_SCHEMA_VERSION: u32 = 2;
pub(crate) const PORTRAIT_COLLECTOR_VERSION: &str = "cognitive-portrait-collector-v2";
pub(crate) const PORTRAIT_CLAIM_CATALOG_VERSION: u32 = 2;
pub(crate) const PORTRAIT_PROJECTION_POLICY: &str = "stratified-provenance-v1";
pub(crate) const MAX_PROJECTED_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_WINDOW_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_WINDOW_DIMENSIONS_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_CLAIM_CATALOG_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_SELECTED_EVIDENCE_PER_WINDOW: usize = 2048;
pub(crate) const MAX_DIMENSION_ENTRIES: usize = 128;
pub(crate) const MAX_DIMENSION_EVIDENCE_IDS: usize = 4;
pub(crate) const MAX_BREAKDOWN_ENTRIES: usize = 128;
pub(crate) const MAX_TOP_PROJECT_STRATA: usize = 32;
pub(crate) const MAX_PROJECTION_TEXT_BYTES: usize = 512;
const PORTRAIT_PROMPT_IDENTITY: &str = "cognitive-portrait-v4:evidence-bundle-v2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CognitivePortraitBundle {
    pub schema_version: u32,
    pub collector_version: String,
    pub period_days: usize,
    pub cutoff: DateTime<Utc>,
    pub manifest: InsightsManifest,
    pub comparison: ComparisonContract,
    pub claim_catalog: PortraitClaimCatalog,
    pub current: PortraitWindowData,
    pub previous: PortraitWindowData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitClaimCatalog {
    pub schema_version: u32,
    pub claims: Vec<PortraitClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitClaim {
    pub claim_id: String,
    pub kind: String,
    pub metric: String,
    pub label: String,
    pub unit: String,
    pub windows: Vec<String>,
    pub pointers: Vec<String>,
    pub values: Vec<u64>,
    pub rendered_line: String,
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
    pub evidence_selection: EvidenceSelection,
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
    pub project_ranking: CountBreakdown,
    pub cognitive_levels: BTreeMap<String, usize>,
    pub collaboration_modes: BTreeMap<String, usize>,
    pub tool_frequency: CountBreakdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CountBreakdown {
    pub total_occurrences: usize,
    pub selected_occurrences: usize,
    pub omitted_occurrences: usize,
    pub selected_entries: usize,
    pub full_digest: String,
    pub selection_digest: String,
    pub entries: Vec<CountEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CountEntry {
    pub value: String,
    pub original_bytes: usize,
    pub value_digest: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EvidenceSelection {
    pub policy_version: String,
    pub eligible_observations: usize,
    pub selected_observations: usize,
    pub omitted_observations: usize,
    pub evidence_byte_budget: usize,
    pub full_payload_digest: String,
    pub selection_digest: String,
    pub strata: Vec<SelectionStratum>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectionStratum {
    pub source: String,
    pub category: String,
    pub project_bucket: String,
    pub eligible_observations: usize,
    pub selected_observations: usize,
    pub omitted_observations: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PortraitDimensions {
    pub projects: DimensionProjection,
    pub decisions: DimensionProjection,
    pub bugfixes: DimensionProjection,
    pub knowledge: DimensionProjection,
    pub patterns: DimensionProjection,
    pub architectures: DimensionProjection,
    pub frictions: DimensionProjection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DimensionProjection {
    pub total_occurrences: usize,
    pub selected_occurrences: usize,
    pub omitted_occurrences: usize,
    pub selected_values: usize,
    pub selected_evidence_refs: usize,
    pub full_digest: String,
    pub entries: Vec<DimensionEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DimensionEvidence {
    pub value: String,
    pub original_bytes: usize,
    pub value_digest: String,
    pub support_count: usize,
    pub omitted_evidence_count: usize,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FieldFingerprint {
    pub bytes: usize,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EvidenceFieldFingerprints {
    pub title: FieldFingerprint,
    pub summary: FieldFingerprint,
    pub content: FieldFingerprint,
    pub excerpt: Option<FieldFingerprint>,
    pub tags: FieldFingerprint,
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
    pub display_text: String,
    pub display_text_original_bytes: usize,
    pub display_text_digest: String,
    pub original_fields: EvidenceFieldFingerprints,
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
    let current_cohort = portrait_session_observations(&snapshot.current, &document_sources);
    let previous_cohort = portrait_session_observations(&snapshot.previous, &document_sources);
    if current_cohort.data_quality.eligible_observations == 0 {
        bail!(
            "NO_CORE_DATA: current rolling window contains no eligible linked session observations"
        );
    }

    let current_manifest = build_window_manifest_from_refs(
        EventTimeWindow {
            start: Some(current_start),
            end: Some(cutoff),
        },
        &current_cohort.eligible_items,
        &snapshot.current,
        &current_cohort.data_quality,
        &snapshot.documents,
    )?;
    let previous_manifest = build_window_manifest_from_refs(
        EventTimeWindow {
            start: Some(previous_start),
            end: Some(current_start),
        },
        &previous_cohort.eligible_items,
        &snapshot.previous,
        &previous_cohort.data_quality,
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
    let current = build_window_data(&current_cohort, &snapshot.documents)?;
    let previous = build_window_data(&previous_cohort, &snapshot.documents)?;
    let claim_catalog = build_claim_catalog(
        &current,
        &previous,
        &manifest.current_window,
        manifest
            .previous_window
            .as_ref()
            .context("previous window manifest is required")?,
        comparison.comparable,
    )?;
    let bundle = CognitivePortraitBundle {
        schema_version: PORTRAIT_BUNDLE_SCHEMA_VERSION,
        collector_version: PORTRAIT_COLLECTOR_VERSION.to_string(),
        period_days,
        cutoff,
        manifest,
        comparison,
        claim_catalog,
        current,
        previous,
    };
    enforce_bundle_budgets(&bundle)?;
    Ok(bundle)
}

fn build_claim_catalog(
    current: &PortraitWindowData,
    previous: &PortraitWindowData,
    current_manifest: &WindowManifest,
    previous_manifest: &WindowManifest,
    comparable: bool,
) -> Result<PortraitClaimCatalog> {
    let metrics = [
        ("total_sessions", "会话总量", "session"),
        ("total_decisions", "决策总量", "decision"),
        ("total_bugfixes", "修复总量", "bugfix"),
        ("total_summaries", "总结总量", "summary"),
        ("untagged_observations", "未标记观察总量", "observation"),
    ];
    let mut claims = Vec::new();
    for (metric, label, unit) in metrics {
        let current_value = metric_value(&current.metrics, metric)?;
        let previous_value = metric_value(&previous.metrics, metric)?;
        for (window, window_label, value) in [
            ("current", "当前窗口", current_value),
            ("previous", "上一窗口", previous_value),
        ] {
            let claim_id = format!("fact.{window}.{metric}");
            claims.push(PortraitClaim {
                claim_id: claim_id.clone(),
                kind: "fact".to_string(),
                metric: metric.to_string(),
                label: label.to_string(),
                unit: unit.to_string(),
                windows: vec![window.to_string()],
                pointers: vec![format!("/{window}/metrics/{metric}")],
                values: vec![u64::try_from(value).context("portrait metric exceeds u64")?],
                rendered_line: format!(
                    "[事实][claim:{claim_id}] {window_label}{label}：{value} {unit}。"
                ),
            });
        }
        if comparable {
            let claim_id = format!("trend.{metric}");
            claims.push(PortraitClaim {
                claim_id: claim_id.clone(),
                kind: "trend".to_string(),
                metric: metric.to_string(),
                label: label.to_string(),
                unit: unit.to_string(),
                windows: vec!["previous".to_string(), "current".to_string()],
                pointers: vec![
                    format!("/previous/metrics/{metric}"),
                    format!("/current/metrics/{metric}"),
                ],
                values: vec![
                    u64::try_from(previous_value).context("portrait metric exceeds u64")?,
                    u64::try_from(current_value).context("portrait metric exceeds u64")?,
                ],
                rendered_line: format!(
                    "[事实][趋势][claim:{claim_id}] {label}：previous={previous_value} {unit}; current={current_value} {unit}。"
                ),
            });
        }
    }
    for (window, window_label, data, window_manifest) in [
        ("current", "当前窗口", current, current_manifest),
        ("previous", "上一窗口", previous, previous_manifest),
    ] {
        add_source_manifest_claims(&mut claims, window, window_label, window_manifest)?;
        add_projection_claims(&mut claims, window, window_label, data)?;
        for (index, _) in data.evidence.iter().enumerate() {
            let claim_id = format!("fact.{window}.evidence.{index:06}");
            let pointer = format!("/{window}/evidence/{index}");
            claims.push(PortraitClaim {
                claim_id: claim_id.clone(),
                kind: "evidence".to_string(),
                metric: "evidence_record".to_string(),
                label: "证据记录".to_string(),
                unit: String::new(),
                windows: vec![window.to_string()],
                pointers: vec![pointer.clone()],
                values: Vec::new(),
                rendered_line: format!(
                    "[事实][claim:{claim_id}] {window_label}证据记录。[bundle:{pointer}]"
                ),
            });
        }
    }
    claims.sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
    Ok(PortraitClaimCatalog {
        schema_version: PORTRAIT_CLAIM_CATALOG_VERSION,
        claims,
    })
}

fn add_source_manifest_claims(
    claims: &mut Vec<PortraitClaim>,
    window: &str,
    window_label: &str,
    manifest: &WindowManifest,
) -> Result<()> {
    let manifest_window = format!("{window}_window");
    for (source, source_label) in [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("platform_unknown", "platform-unknown"),
    ] {
        let stats = manifest
            .source_counts
            .iter()
            .enumerate()
            .find(|(_, stats)| stats.source == source);
        let (observations, sessions, freshness, pointers) = if let Some((index, stats)) = stats {
            let prefix = format!("/manifest/{manifest_window}/source_counts/{index}");
            (
                stats.observation_count,
                stats.session_count,
                format_freshness(stats.freshest_event_time),
                vec![
                    format!("{prefix}/observation_count"),
                    format!("{prefix}/session_count"),
                    format!("{prefix}/freshest_event_time"),
                ],
            )
        } else {
            (
                0,
                0,
                "unavailable".to_string(),
                vec![format!("/manifest/{manifest_window}/source_counts")],
            )
        };
        let claim_id = format!("fact.{window}.manifest.source.{source}.coverage");
        claims.push(PortraitClaim {
            claim_id: claim_id.clone(),
            kind: "fact".to_string(),
            metric: format!("manifest.source.{source}.coverage"),
            label: format!("{source_label} 来源覆盖与新鲜度"),
            unit: "observation,session,timestamp".to_string(),
            windows: vec![window.to_string()],
            pointers,
            values: vec![
                u64::try_from(observations).context("source observation count exceeds u64")?,
                u64::try_from(sessions).context("source session count exceeds u64")?,
            ],
            rendered_line: format!(
                "[事实][claim:{claim_id}] {window_label}{source_label} 来源：observations={observations} observation; sessions={sessions} session; freshest_event_time={freshness}。"
            ),
        });
    }

    let unsupported = &manifest.unsupported_sources;
    let claim_id = format!("fact.{window}.manifest.unsupported_sources.coverage");
    claims.push(PortraitClaim {
        claim_id: claim_id.clone(),
        kind: "fact".to_string(),
        metric: "manifest.unsupported_sources.coverage".to_string(),
        label: "unsupported 来源覆盖与新鲜度".to_string(),
        unit: "observation,session,timestamp".to_string(),
        windows: vec![window.to_string()],
        pointers: vec![
            format!("/manifest/{manifest_window}/unsupported_sources/total_observations"),
            format!("/manifest/{manifest_window}/unsupported_sources/total_sessions"),
            format!("/manifest/{manifest_window}/unsupported_sources/freshest_event_time"),
        ],
        values: vec![
            u64::try_from(unsupported.total_observations)
                .context("unsupported source observation count exceeds u64")?,
            u64::try_from(unsupported.total_sessions)
                .context("unsupported source session count exceeds u64")?,
        ],
        rendered_line: format!(
            "[事实][claim:{claim_id}] {window_label}unsupported 来源：observations={} observation; sessions={} session; freshest_event_time={}。",
            unsupported.total_observations,
            unsupported.total_sessions,
            format_freshness(unsupported.freshest_event_time),
        ),
    });
    Ok(())
}

fn format_freshness(freshness: Option<DateTime<Utc>>) -> String {
    freshness
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "unavailable".to_string())
}

fn add_projection_claims(
    claims: &mut Vec<PortraitClaim>,
    window: &str,
    window_label: &str,
    data: &PortraitWindowData,
) -> Result<()> {
    for (metric, label, unit, pointer, value) in [
        (
            "evidence_selection.eligible_observations",
            "可用观察总量",
            "observation",
            format!("/{window}/evidence_selection/eligible_observations"),
            data.evidence_selection.eligible_observations,
        ),
        (
            "evidence_selection.selected_observations",
            "保留证据观察量",
            "observation",
            format!("/{window}/evidence_selection/selected_observations"),
            data.evidence_selection.selected_observations,
        ),
        (
            "evidence_selection.omitted_observations",
            "省略证据观察量",
            "observation",
            format!("/{window}/evidence_selection/omitted_observations"),
            data.evidence_selection.omitted_observations,
        ),
    ] {
        push_projection_claim(
            claims,
            window,
            window_label,
            metric,
            label,
            unit,
            pointer,
            value,
        )?;
    }
    for (name, label, dimension) in [
        ("projects", "项目维度", &data.dimensions.projects),
        ("decisions", "决策维度", &data.dimensions.decisions),
        ("bugfixes", "修复维度", &data.dimensions.bugfixes),
        ("knowledge", "知识维度", &data.dimensions.knowledge),
        ("patterns", "模式维度", &data.dimensions.patterns),
        ("architectures", "架构维度", &data.dimensions.architectures),
        ("frictions", "阻力维度", &data.dimensions.frictions),
    ] {
        for (field, field_label, unit, value) in [
            (
                "total_occurrences",
                "完整 occurrence 总量",
                "occurrence",
                dimension.total_occurrences,
            ),
            (
                "selected_occurrences",
                "保留 occurrence 总量",
                "occurrence",
                dimension.selected_occurrences,
            ),
            (
                "omitted_occurrences",
                "省略 occurrence 总量",
                "occurrence",
                dimension.omitted_occurrences,
            ),
            (
                "selected_values",
                "保留值总量",
                "value",
                dimension.selected_values,
            ),
            (
                "selected_evidence_refs",
                "保留证据引用总量",
                "reference",
                dimension.selected_evidence_refs,
            ),
        ] {
            let metric = format!("dimensions.{name}.{field}");
            push_projection_claim(
                claims,
                window,
                window_label,
                &metric,
                &format!("{label}{field_label}"),
                unit,
                format!("/{window}/dimensions/{name}/{field}"),
                value,
            )?;
        }
    }
    for (name, label, breakdown) in [
        ("project_ranking", "项目排名", &data.metrics.project_ranking),
        ("tool_frequency", "工具频率", &data.metrics.tool_frequency),
    ] {
        for (field, field_label, unit, value) in [
            (
                "total_occurrences",
                "完整 occurrence 总量",
                "occurrence",
                breakdown.total_occurrences,
            ),
            (
                "selected_occurrences",
                "保留 occurrence 总量",
                "occurrence",
                breakdown.selected_occurrences,
            ),
            (
                "omitted_occurrences",
                "省略 occurrence 总量",
                "occurrence",
                breakdown.omitted_occurrences,
            ),
            (
                "selected_entries",
                "保留条目总量",
                "entry",
                breakdown.selected_entries,
            ),
        ] {
            let metric = format!("metrics.{name}.{field}");
            push_projection_claim(
                claims,
                window,
                window_label,
                &metric,
                &format!("{label}{field_label}"),
                unit,
                format!("/{window}/metrics/{name}/{field}"),
                value,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_projection_claim(
    claims: &mut Vec<PortraitClaim>,
    window: &str,
    window_label: &str,
    metric: &str,
    label: &str,
    unit: &str,
    pointer: String,
    value: usize,
) -> Result<()> {
    let claim_id = format!("fact.{window}.{metric}");
    claims.push(PortraitClaim {
        claim_id: claim_id.clone(),
        kind: "fact".to_string(),
        metric: metric.to_string(),
        label: label.to_string(),
        unit: unit.to_string(),
        windows: vec![window.to_string()],
        pointers: vec![pointer],
        values: vec![u64::try_from(value).context("portrait projection count exceeds u64")?],
        rendered_line: format!("[事实][claim:{claim_id}] {window_label}{label}：{value} {unit}。"),
    });
    Ok(())
}

fn metric_value(metrics: &PortraitMetrics, metric: &str) -> Result<usize> {
    match metric {
        "total_sessions" => Ok(metrics.total_sessions),
        "total_decisions" => Ok(metrics.total_decisions),
        "total_bugfixes" => Ok(metrics.total_bugfixes),
        "total_summaries" => Ok(metrics.total_summaries),
        "untagged_observations" => Ok(metrics.untagged_observations),
        _ => bail!("SCHEMA_INVALID: unsupported portrait claim metric {metric}"),
    }
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

pub(crate) fn write_bundle(path: &Path, bundle: &CognitivePortraitBundle) -> Result<()> {
    enforce_bundle_budgets(bundle)?;
    let mut json = Vec::with_capacity(1024 * 1024);
    serde_json::to_writer_pretty(&mut json, bundle).context("serialize portrait bundle")?;
    json.push(b'\n');
    if json.len() > MAX_PORTRAIT_BUNDLE_BYTES {
        bail!(
            "DATA_QUALITY_DEGRADED: cognitive portrait bundle exceeds the {} byte limit",
            MAX_PORTRAIT_BUNDLE_BYTES
        );
    }
    fs::write(path, &json)
        .with_context(|| format!("write cognitive portrait bundle {}", path.display()))
}

pub(crate) fn read_bundle(path: &Path) -> Result<CognitivePortraitBundle> {
    let raw = read_utf8_bounded(path, MAX_PORTRAIT_BUNDLE_BYTES, "cognitive portrait bundle")?;
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
    if bundle.manifest.manifest_version != crate::insights_manifest::MANIFEST_VERSION {
        bail!(
            "SCHEMA_INVALID: unsupported insights manifest schema {}; expected {}",
            bundle.manifest.manifest_version,
            crate::insights_manifest::MANIFEST_VERSION
        );
    }
    if bundle.period_days == 0 || bundle.manifest.event_time_cutoff != bundle.cutoff {
        bail!("SCHEMA_INVALID: bundle cutoff/period invariant failed");
    }
    let previous_manifest = bundle
        .manifest
        .previous_window
        .as_ref()
        .context("SCHEMA_INVALID: previous window manifest is required")?;
    if bundle.manifest.current_window.eligible_observations
        != bundle.current.evidence_selection.eligible_observations
        || previous_manifest.eligible_observations
            != bundle.previous.evidence_selection.eligible_observations
    {
        bail!("SCHEMA_INVALID: manifest, cohort, and projection counts disagree");
    }
    if bundle.comparison != comparison_contract(&bundle.manifest.current_window, previous_manifest)
    {
        bail!("SCHEMA_INVALID: comparison contract disagrees with window manifests");
    }
    validate_window_manifest(&bundle.manifest.current_window, "current")?;
    validate_window_manifest(previous_manifest, "previous")?;
    validate_window_projection(&bundle.current, "current")?;
    validate_window_projection(&bundle.previous, "previous")?;
    let expected_catalog = build_claim_catalog(
        &bundle.current,
        &bundle.previous,
        &bundle.manifest.current_window,
        previous_manifest,
        bundle.comparison.comparable,
    )?;
    if bundle.claim_catalog != expected_catalog {
        bail!(
            "SCHEMA_INVALID: claim catalog disagrees with trusted metrics or canonical rendering"
        );
    }
    enforce_bundle_budgets(&bundle)?;
    Ok(bundle)
}
