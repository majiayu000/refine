mod hashing;
mod validate;

use crate::insights_manifest::report_source;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::knowledge::{Item, ObservationDocumentMeta};
use refine_core::session::{eligible_observations, ClusterResult};
use std::collections::{BTreeMap, BTreeSet};

use self::hashing::{
    fingerprint, sha256_bytes, sha256_json, truncate_projection_text, StableDigest,
};
use super::bundle::{
    CountBreakdown, CountEntry, DimensionEvidence, DimensionProjection, EvidenceFieldFingerprints,
    EvidenceRecord, EvidenceSelection, PortraitDimensions, PortraitMetrics, PortraitWindowData,
    SelectionStratum, MAX_BREAKDOWN_ENTRIES, MAX_DIMENSION_ENTRIES, MAX_DIMENSION_EVIDENCE_IDS,
    MAX_SELECTED_EVIDENCE_PER_WINDOW, MAX_TOP_PROJECT_STRATA, MAX_WINDOW_EVIDENCE_BYTES,
    PORTRAIT_PROJECTION_POLICY,
};

pub(super) use validate::{enforce_bundle_budgets, validate_window_projection};

#[derive(Debug)]
struct Candidate {
    record: EvidenceRecord,
    primary_category: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct StratumKey {
    source: String,
    category: String,
    project_bucket: String,
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

#[derive(Default)]
struct EvidenceValues(BTreeMap<String, BTreeSet<String>>);

impl EvidenceValues {
    fn add(&mut self, value: &str, evidence_id: &str) {
        self.0
            .entry(value.to_string())
            .or_default()
            .insert(evidence_id.to_string());
    }
}

pub(super) fn build_window_data(
    cohort_items: &[Item],
    cluster: &ClusterResult,
    documents: &[ObservationDocumentMeta],
) -> Result<PortraitWindowData> {
    let documents: BTreeMap<&str, &ObservationDocumentMeta> = documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect();
    let eligible = eligible_observations(cohort_items);
    let top_projects: BTreeSet<String> = sorted_project_counts(cluster)
        .into_iter()
        .take(MAX_TOP_PROJECT_STRATA)
        .map(|(project, _)| project)
        .collect();
    let mut dimensions = DimensionAccumulator::default();
    let mut candidates = Vec::with_capacity(eligible.len());

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
        let project = cluster
            .item_projects
            .get(item.id().as_str())
            .with_context(|| {
                format!(
                    "eligible portrait observation is missing its direct project assignment {}",
                    item.id()
                )
            })?
            .clone();
        let evidence_id = format!("obs:{}", item.id());
        let tags: Vec<String> = item
            .tags()
            .iter()
            .map(|tag| tag.as_str().to_string())
            .collect();
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).collect();
        let mut categories = BTreeSet::new();
        let mut preferred_display: Option<String> = None;

        dimensions.projects.add(&project, &evidence_id);
        if tag_refs.contains(&"decision") {
            categories.insert("decision".to_string());
            dimensions.decisions.add(item.title(), &evidence_id);
            preferred_display = Some(item.title().to_string());
        } else if tag_refs.contains(&"bugfix") {
            categories.insert("bugfix".to_string());
            dimensions.bugfixes.add(item.title(), &evidence_id);
            preferred_display = Some(item.title().to_string());
        } else {
            categories.insert("summary".to_string());
        }

        add_sections(
            item,
            &evidence_id,
            &mut categories,
            &mut preferred_display,
            &mut dimensions,
        );
        let source = report_source(&document.source).to_string();
        let categories: Vec<String> = categories.into_iter().collect();
        let primary_category = primary_category(&categories).to_string();
        let display_source = preferred_display.unwrap_or_else(|| item.title().to_string());
        let tags_json = serde_json::to_vec(&tags).context("serialize portrait tags")?;
        candidates.push(Candidate {
            primary_category,
            record: EvidenceRecord {
                evidence_id,
                item_id: item.id().as_str().to_string(),
                document_id: document_id.as_str().to_string(),
                event_time: document.captured_at,
                source,
                project,
                categories,
                display_text: truncate_projection_text(&display_source),
                display_text_original_bytes: display_source.len(),
                display_text_digest: sha256_bytes(display_source.as_bytes()),
                original_fields: EvidenceFieldFingerprints {
                    title: fingerprint(item.title().as_bytes()),
                    summary: fingerprint(item.summary().as_bytes()),
                    content: fingerprint(item.content().as_bytes()),
                    excerpt: item.excerpt().map(|value| fingerprint(value.as_bytes())),
                    tags: fingerprint(&tags_json),
                },
            },
        });
    }

    candidates.sort_by(|left, right| {
        left.record
            .event_time
            .cmp(&right.record.event_time)
            .then_with(|| left.record.item_id.cmp(&right.record.item_id))
    });
    let full_payload_digest = full_payload_digest(&candidates)?;
    let (evidence, strata) = select_evidence(candidates, &top_projects);
    let selected_ids: BTreeSet<String> = evidence
        .iter()
        .map(|record| record.evidence_id.clone())
        .collect();
    let event_times: BTreeMap<String, DateTime<Utc>> = evidence
        .iter()
        .map(|record| (record.evidence_id.clone(), record.event_time))
        .collect();
    let dimensions = finish_dimensions(dimensions, &selected_ids, &event_times)?;
    let selection_digest = selection_digest(&evidence, &dimensions, &strata)?;
    let eligible_observations = cluster.data_quality.eligible_observations;
    let selected_observations = evidence.len();
    let omitted_observations = eligible_observations
        .checked_sub(selected_observations)
        .context("projection selected more observations than the eligible cohort")?;
    let stats = &cluster.global_stats;
    let data = PortraitWindowData {
        metrics: PortraitMetrics {
            total_sessions: stats.total_sessions,
            total_decisions: stats.total_decisions,
            total_bugfixes: stats.total_bugfixes,
            total_summaries: stats.total_summaries,
            untagged_observations: cluster.untagged_count,
            project_ranking: build_count_breakdown(sorted_project_counts(cluster))?,
            cognitive_levels: stats.cognitive_levels.clone().into_iter().collect(),
            collaboration_modes: stats.collaboration_modes.clone().into_iter().collect(),
            tool_frequency: build_count_breakdown(
                stats
                    .tool_frequency
                    .iter()
                    .map(|(value, count)| (value.clone(), *count))
                    .collect(),
            )?,
        },
        evidence_selection: EvidenceSelection {
            policy_version: PORTRAIT_PROJECTION_POLICY.to_string(),
            eligible_observations,
            selected_observations,
            omitted_observations,
            evidence_byte_budget: MAX_WINDOW_EVIDENCE_BYTES,
            full_payload_digest,
            selection_digest,
            strata,
        },
        evidence,
        dimensions,
    };
    validate_window_projection(&data, "collector")?;
    Ok(data)
}

fn add_sections(
    item: &Item,
    evidence_id: &str,
    categories: &mut BTreeSet<String>,
    preferred_display: &mut Option<String>,
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
            if preferred_display.is_none() {
                *preferred_display = Some(value.clone());
            }
            target.add(&value, evidence_id);
        }
    }
}

fn sorted_project_counts(cluster: &ClusterResult) -> Vec<(String, usize)> {
    let mut entries = cluster.global_stats.project_ranking.clone();
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    entries
}

fn build_count_breakdown(mut entries: Vec<(String, usize)>) -> Result<CountBreakdown> {
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let full_digest = count_breakdown_digest(&entries);
    let total_entries = entries.len();
    let selected: Vec<CountEntry> = entries
        .into_iter()
        .take(MAX_BREAKDOWN_ENTRIES)
        .map(|(value, count)| CountEntry {
            value: truncate_projection_text(&value),
            original_bytes: value.len(),
            value_digest: sha256_bytes(value.as_bytes()),
            count,
        })
        .collect();
    let selected_entries = selected.len();
    let selection_digest = sha256_json(&selected)?;
    Ok(CountBreakdown {
        total_entries,
        selected_entries,
        omitted_entries: total_entries - selected_entries,
        full_digest,
        selection_digest,
        entries: selected,
    })
}

fn select_evidence(
    candidates: Vec<Candidate>,
    top_projects: &BTreeSet<String>,
) -> (Vec<EvidenceRecord>, Vec<SelectionStratum>) {
    let mut groups: BTreeMap<StratumKey, Vec<usize>> = BTreeMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let project_bucket = if top_projects.contains(&candidate.record.project) {
            candidate.record.project.clone()
        } else {
            "__other__".to_string()
        };
        groups
            .entry(StratumKey {
                source: candidate.record.source.clone(),
                category: candidate.primary_category.clone(),
                project_bucket,
            })
            .or_default()
            .push(index);
    }
    for indices in groups.values_mut() {
        indices.sort_by(|left, right| {
            candidates[*right]
                .record
                .event_time
                .cmp(&candidates[*left].record.event_time)
                .then_with(|| {
                    candidates[*left]
                        .record
                        .evidence_id
                        .cmp(&candidates[*right].record.evidence_id)
                })
        });
    }

    let limit = candidates.len().min(MAX_SELECTED_EVIDENCE_PER_WINDOW);
    let mut offsets: BTreeMap<StratumKey, usize> =
        groups.keys().cloned().map(|key| (key, 0)).collect();
    let mut selected_indices = Vec::with_capacity(limit);
    while selected_indices.len() < limit {
        let mut progressed = false;
        for (key, indices) in &groups {
            if selected_indices.len() == limit {
                break;
            }
            let offset = offsets.get_mut(key).expect("stratum offset exists");
            if let Some(index) = indices.get(*offset) {
                selected_indices.push(*index);
                *offset += 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    selected_indices.sort_by(|left, right| {
        candidates[*left]
            .record
            .event_time
            .cmp(&candidates[*right].record.event_time)
            .then_with(|| {
                candidates[*left]
                    .record
                    .item_id
                    .cmp(&candidates[*right].record.item_id)
            })
    });
    let selected_set: BTreeSet<usize> = selected_indices.iter().copied().collect();
    let evidence = selected_indices
        .into_iter()
        .map(|index| candidates[index].record.clone())
        .collect();
    let strata = groups
        .into_iter()
        .map(|(key, indices)| {
            let selected_observations = indices
                .iter()
                .filter(|index| selected_set.contains(index))
                .count();
            SelectionStratum {
                source: key.source,
                category: key.category,
                project_bucket: key.project_bucket,
                eligible_observations: indices.len(),
                selected_observations,
                omitted_observations: indices.len() - selected_observations,
            }
        })
        .collect();
    (evidence, strata)
}

fn finish_dimensions(
    dimensions: DimensionAccumulator,
    selected_ids: &BTreeSet<String>,
    event_times: &BTreeMap<String, DateTime<Utc>>,
) -> Result<PortraitDimensions> {
    Ok(PortraitDimensions {
        projects: finish_dimension(dimensions.projects, selected_ids, event_times)?,
        decisions: finish_dimension(dimensions.decisions, selected_ids, event_times)?,
        bugfixes: finish_dimension(dimensions.bugfixes, selected_ids, event_times)?,
        knowledge: finish_dimension(dimensions.knowledge, selected_ids, event_times)?,
        patterns: finish_dimension(dimensions.patterns, selected_ids, event_times)?,
        architectures: finish_dimension(dimensions.architectures, selected_ids, event_times)?,
        frictions: finish_dimension(dimensions.frictions, selected_ids, event_times)?,
    })
}

fn finish_dimension(
    values: EvidenceValues,
    selected_ids: &BTreeSet<String>,
    event_times: &BTreeMap<String, DateTime<Utc>>,
) -> Result<DimensionProjection> {
    let full_digest = dimension_digest(&values.0);
    let total_values = values.0.len();
    let total_evidence_refs: usize = values.0.values().map(BTreeSet::len).sum();
    let mut candidates = Vec::new();
    for (value, evidence_ids) in values.0 {
        let mut retained: Vec<String> = evidence_ids
            .iter()
            .filter(|id| selected_ids.contains(*id))
            .cloned()
            .collect();
        if retained.is_empty() {
            continue;
        }
        retained.sort_by(|left, right| {
            event_times
                .get(right)
                .cmp(&event_times.get(left))
                .then_with(|| left.cmp(right))
        });
        let latest = *event_times
            .get(&retained[0])
            .expect("retained evidence event exists");
        candidates.push((value, evidence_ids.len(), latest, retained));
    }
    candidates.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| left.0.cmp(&right.0))
    });
    let entries: Vec<DimensionEvidence> = candidates
        .into_iter()
        .take(MAX_DIMENSION_ENTRIES)
        .map(|(value, support_count, _, evidence_ids)| {
            let evidence_ids: Vec<String> = evidence_ids
                .into_iter()
                .take(MAX_DIMENSION_EVIDENCE_IDS)
                .collect();
            DimensionEvidence {
                value: truncate_projection_text(&value),
                original_bytes: value.len(),
                value_digest: sha256_bytes(value.as_bytes()),
                support_count,
                omitted_evidence_count: support_count - evidence_ids.len(),
                evidence_ids,
            }
        })
        .collect();
    let selected_values = entries.len();
    let selected_evidence_refs = entries.iter().map(|entry| entry.evidence_ids.len()).sum();
    Ok(DimensionProjection {
        total_values,
        selected_values,
        omitted_values: total_values - selected_values,
        total_evidence_refs,
        selected_evidence_refs,
        omitted_evidence_refs: total_evidence_refs - selected_evidence_refs,
        full_digest,
        entries,
    })
}

pub(super) fn primary_category(categories: &[String]) -> &str {
    for category in [
        "decision",
        "bugfix",
        "knowledge",
        "pattern",
        "architecture",
        "friction",
        "summary",
    ] {
        if categories.iter().any(|candidate| candidate == category) {
            return category;
        }
    }
    "summary"
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

fn full_payload_digest(candidates: &[Candidate]) -> Result<String> {
    let mut digest = StableDigest::new("cognitive-portrait-full-payload-v2");
    digest.usize(candidates.len());
    for candidate in candidates {
        let record = &candidate.record;
        digest.text(&record.evidence_id);
        digest.text(&record.item_id);
        digest.text(&record.document_id);
        digest.text(&record.event_time.to_rfc3339());
        digest.text(&record.source);
        digest.text(&record.project);
        digest.usize(record.categories.len());
        for category in &record.categories {
            digest.text(category);
        }
        digest_fingerprint(&mut digest, &record.original_fields.title);
        digest_fingerprint(&mut digest, &record.original_fields.summary);
        digest_fingerprint(&mut digest, &record.original_fields.content);
        match &record.original_fields.excerpt {
            Some(value) => {
                digest.text("some");
                digest_fingerprint(&mut digest, value);
            }
            None => digest.text("none"),
        }
        digest_fingerprint(&mut digest, &record.original_fields.tags);
    }
    Ok(digest.finish())
}

fn count_breakdown_digest(entries: &[(String, usize)]) -> String {
    let mut digest = StableDigest::new("cognitive-portrait-count-breakdown-v2");
    digest.usize(entries.len());
    for (value, count) in entries {
        digest.text(value);
        digest.usize(*count);
    }
    digest.finish()
}

fn dimension_digest(values: &BTreeMap<String, BTreeSet<String>>) -> String {
    let mut digest = StableDigest::new("cognitive-portrait-dimension-v2");
    digest.usize(values.len());
    for (value, evidence_ids) in values {
        digest.text(value);
        digest.usize(evidence_ids.len());
        for evidence_id in evidence_ids {
            digest.text(evidence_id);
        }
    }
    digest.finish()
}

fn digest_fingerprint(digest: &mut StableDigest, value: &super::bundle::FieldFingerprint) {
    digest.usize(value.bytes);
    digest.text(&value.digest);
}

pub(super) fn selection_digest(
    evidence: &[EvidenceRecord],
    dimensions: &PortraitDimensions,
    strata: &[SelectionStratum],
) -> Result<String> {
    sha256_json(&(PORTRAIT_PROJECTION_POLICY, evidence, dimensions, strata))
}
