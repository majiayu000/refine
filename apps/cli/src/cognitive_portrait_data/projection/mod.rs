mod dimensions;
pub(crate) mod hashing;
mod selection;
mod validate;

use crate::insights_manifest::report_source;
use anyhow::{Context, Result};
use refine_core::knowledge::{Item, ObservationDocumentMeta};
use refine_core::session::PortraitSessionCohort;
use std::collections::{BTreeMap, BTreeSet};

use self::dimensions::DimensionAccumulators;
use self::hashing::{
    fingerprint, sha256_bytes, sha256_json, truncate_projection_text, MultisetDigest, StableDigest,
};
use self::selection::{
    allocate_quotas, build_strata, next_evidence_json_bytes, BoundedSelection, StratumKey,
};
use super::bundle::{
    CountBreakdown, CountEntry, EvidenceFieldFingerprints, EvidenceRecord, EvidenceSelection,
    PortraitDimensions, PortraitMetrics, PortraitWindowData, MAX_BREAKDOWN_ENTRIES,
    MAX_TOP_PROJECT_STRATA, MAX_WINDOW_EVIDENCE_BYTES, PORTRAIT_PROJECTION_POLICY,
};

pub(super) use validate::{enforce_bundle_budgets, validate_window_projection};

pub(super) fn build_window_data(
    cohort: &PortraitSessionCohort<'_>,
    documents: &[ObservationDocumentMeta],
) -> Result<PortraitWindowData> {
    let documents: BTreeMap<&str, &ObservationDocumentMeta> = documents
        .iter()
        .map(|document| (document.id.as_str(), document))
        .collect();
    let eligible = &cohort.eligible_items;
    let top_projects: BTreeSet<String> = cohort
        .global_stats
        .project_ranking
        .iter()
        .take(MAX_TOP_PROJECT_STRATA)
        .map(|(project, _)| project.clone())
        .collect();

    // Pass 1 stores only bounded stratum counters and a commutative digest.
    let mut stratum_counts = BTreeMap::new();
    let mut payload_digest = MultisetDigest::default();
    for (index, item) in eligible.iter().enumerate() {
        let document = item_document(item, &documents)?;
        let project = &cohort.item_projects[index];
        let key = stratum_key(item, document, project, &top_projects);
        *stratum_counts.entry(key).or_default() += 1;
        payload_digest.add(payload_row_digest(item, document, project));
    }
    let full_payload_digest = payload_digest.finish("cognitive-portrait-full-payload-v2");

    // Pass 2 retains at most 2,048 indices across deterministic strata.
    let mut selector = BoundedSelection::new(allocate_quotas(&stratum_counts));
    for (index, item) in eligible.iter().enumerate() {
        let document = item_document(item, &documents)?;
        let project = &cohort.item_projects[index];
        selector.consider(
            stratum_key(item, document, project, &top_projects),
            index,
            document.captured_at,
            item.id().as_str(),
        );
    }

    // Build only retained records. Round-robin packing uses exact compact JSON
    // bytes, including escaping and separators, before accepting each record.
    let ranked = selector.into_ranked();
    let mut offsets = vec![0usize; ranked.len()];
    let mut evidence = Vec::new();
    let mut selected_counts = BTreeMap::new();
    let mut selected_indices = BTreeSet::new();
    let mut evidence_json_bytes = 2usize;
    loop {
        let mut progressed = false;
        for (group_index, (key, indices)) in ranked.iter().enumerate() {
            let offset = &mut offsets[group_index];
            let Some(index) = indices.get(*offset).copied() else {
                continue;
            };
            *offset += 1;
            progressed = true;
            let item = eligible[index];
            let document = item_document(item, &documents)?;
            let project = &cohort.item_projects[index];
            let record = build_evidence_record(item, document, project)?;
            let record_bytes = serde_json::to_vec(&record)
                .context("serialize portrait evidence record for byte budget")?
                .len();
            if let Some(next_bytes) = next_evidence_json_bytes(
                evidence_json_bytes,
                evidence.len(),
                record_bytes,
                MAX_WINDOW_EVIDENCE_BYTES,
            ) {
                evidence_json_bytes = next_bytes;
                evidence.push(record);
                selected_indices.insert(index);
                *selected_counts.entry(key.clone()).or_default() += 1;
            }
        }
        if !progressed {
            break;
        }
    }
    evidence.sort_by(|left, right| {
        left.event_time
            .cmp(&right.event_time)
            .then_with(|| left.item_id.cmp(&right.item_id))
    });
    debug_assert_eq!(serde_json::to_vec(&evidence)?.len(), evidence_json_bytes);
    let strata = build_strata(stratum_counts, &selected_counts);

    // Passes 3 and 4 keep only 128 min-hash keys per dimension, then recount
    // exact occurrence support and provenance against the full cohort.
    let mut dimension_accumulators = DimensionAccumulators::default();
    for index in &selected_indices {
        let item = eligible[*index];
        let project = &cohort.item_projects[*index];
        dimension_accumulators.sample_item(item, project);
    }
    for (index, item) in eligible.iter().enumerate() {
        let document = item_document(item, &documents)?;
        let project = &cohort.item_projects[index];
        dimension_accumulators.observe_item(
            item,
            project,
            document.captured_at,
            selected_indices.contains(&index),
        );
    }
    let dimensions = dimension_accumulators.finish();
    let selection_digest =
        selection_digest(&evidence, &dimensions, &strata, MAX_WINDOW_EVIDENCE_BYTES)?;
    let eligible_observations = cohort.data_quality.eligible_observations;
    let selected_observations = evidence.len();
    let omitted_observations = eligible_observations
        .checked_sub(selected_observations)
        .context("projection selected more observations than the eligible cohort")?;
    let stats = &cohort.global_stats;
    let data = PortraitWindowData {
        metrics: PortraitMetrics {
            total_sessions: stats.total_sessions,
            total_decisions: stats.total_decisions,
            total_bugfixes: stats.total_bugfixes,
            total_summaries: stats.total_summaries,
            untagged_observations: cohort.untagged_count,
            project_ranking: build_count_breakdown(
                stats
                    .project_ranking
                    .iter()
                    .map(|(value, count)| (value.as_str(), *count)),
            )?,
            cognitive_levels: stats.cognitive_levels.clone().into_iter().collect(),
            collaboration_modes: stats.collaboration_modes.clone().into_iter().collect(),
            tool_frequency: build_tool_breakdown(eligible)?,
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

fn item_document<'a>(
    item: &Item,
    documents: &BTreeMap<&str, &'a ObservationDocumentMeta>,
) -> Result<&'a ObservationDocumentMeta> {
    let document_id = item
        .document_id()
        .context("eligible portrait observation unexpectedly lacks document_id")?;
    let document = documents
        .get(document_id.as_str())
        .copied()
        .with_context(|| {
            format!(
                "eligible portrait observation references missing document metadata {document_id}"
            )
        })?;
    Ok(document)
}

fn stratum_key(
    item: &Item,
    document: &ObservationDocumentMeta,
    project: &str,
    top_projects: &BTreeSet<String>,
) -> StratumKey {
    StratumKey {
        source: report_source(&document.source).to_string(),
        category: primary_category_for_item(item).to_string(),
        project_bucket: if top_projects.contains(project) {
            project.to_string()
        } else {
            "__other__".to_string()
        },
    }
}

fn primary_category_for_item(item: &Item) -> &'static str {
    if item.tags().iter().any(|tag| tag.as_str() == "decision") {
        "decision"
    } else if item.tags().iter().any(|tag| tag.as_str() == "bugfix") {
        "bugfix"
    } else if has_section_item(item.content(), "知识") {
        "knowledge"
    } else if has_section_item(item.content(), "模式") {
        "pattern"
    } else if has_section_item(item.content(), "架构") {
        "architecture"
    } else if has_section_item(item.content(), "阻力") {
        "friction"
    } else {
        "summary"
    }
}

fn build_evidence_record(
    item: &Item,
    document: &ObservationDocumentMeta,
    project: &str,
) -> Result<EvidenceRecord> {
    let tags: Vec<String> = item
        .tags()
        .iter()
        .map(|tag| tag.as_str().to_string())
        .collect();
    let mut categories = BTreeSet::new();
    if tags.iter().any(|tag| tag == "decision") {
        categories.insert("decision".to_string());
    } else if tags.iter().any(|tag| tag == "bugfix") {
        categories.insert("bugfix".to_string());
    } else {
        categories.insert("summary".to_string());
    }
    let mut display = None;
    for (category, section) in [
        ("knowledge", "知识"),
        ("pattern", "模式"),
        ("architecture", "架构"),
        ("friction", "阻力"),
    ] {
        for_each_section_item(item.content(), section, |value| {
            categories.insert(category.to_string());
            if display.is_none() {
                display = Some(value);
            }
        });
    }
    let display = if categories.contains("decision") || categories.contains("bugfix") {
        item.title()
    } else {
        display.unwrap_or(item.title())
    };
    let mut tags_digest = StableDigest::new("cognitive-portrait-tags-v2");
    tags_digest.usize(tags.len());
    for tag in &tags {
        tags_digest.text(tag);
    }
    Ok(EvidenceRecord {
        evidence_id: format!("obs:{}", item.id()),
        item_id: item.id().as_str().to_string(),
        document_id: item
            .document_id()
            .expect("eligible item has document id")
            .as_str()
            .to_string(),
        event_time: document.captured_at,
        source: report_source(&document.source).to_string(),
        project: project.to_string(),
        categories: categories.into_iter().collect(),
        display_text: truncate_projection_text(display),
        display_text_original_bytes: display.len(),
        display_text_digest: sha256_bytes(display.as_bytes()),
        original_fields: EvidenceFieldFingerprints {
            title: fingerprint(item.title().as_bytes()),
            summary: fingerprint(item.summary().as_bytes()),
            content: fingerprint(item.content().as_bytes()),
            excerpt: item.excerpt().map(|value| fingerprint(value.as_bytes())),
            tags: super::bundle::FieldFingerprint {
                bytes: tags.iter().map(String::len).sum(),
                digest: tags_digest.finish(),
            },
        },
    })
}

fn payload_row_digest(item: &Item, document: &ObservationDocumentMeta, project: &str) -> [u8; 32] {
    let mut row = StableDigest::new("cognitive-portrait-payload-row-v2");
    row.text(item.id().as_str());
    row.text(item.document_id().map_or("", |id| id.as_str()));
    row.text(&document.captured_at.to_rfc3339());
    row.text(report_source(&document.source));
    row.text(project);
    row.text(primary_category_for_item(item));
    row.text(item.title());
    row.text(item.summary());
    row.text(item.content());
    match item.excerpt() {
        Some(excerpt) => {
            row.text("excerpt:some");
            row.text(excerpt);
        }
        None => row.text("excerpt:none"),
    }
    row.usize(item.tags().len());
    for tag in item.tags() {
        row.text(tag.as_str());
    }
    row.finish_bytes()
}

fn build_count_breakdown<'a>(
    entries: impl Iterator<Item = (&'a str, usize)>,
) -> Result<CountBreakdown> {
    let mut full = MultisetDigest::default();
    let mut total_occurrences = 0usize;
    let mut selected = Vec::with_capacity(MAX_BREAKDOWN_ENTRIES);
    for (value, count) in entries {
        total_occurrences = total_occurrences
            .checked_add(count)
            .context("portrait count breakdown total overflow")?;
        let mut row = StableDigest::new("cognitive-portrait-count-row-v2");
        row.text(value);
        row.usize(count);
        full.add(row.finish_bytes());
        selected.push((value, count));
        selected.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        selected.truncate(MAX_BREAKDOWN_ENTRIES);
    }
    let selected: Vec<CountEntry> = selected
        .into_iter()
        .map(|(value, count)| CountEntry {
            value: truncate_projection_text(value),
            original_bytes: value.len(),
            value_digest: sha256_bytes(value.as_bytes()),
            count,
        })
        .collect();
    let selected_occurrences = selected.iter().map(|entry| entry.count).sum();
    Ok(CountBreakdown {
        total_occurrences,
        selected_occurrences,
        omitted_occurrences: total_occurrences - selected_occurrences,
        selected_entries: selected.len(),
        full_digest: full.finish("cognitive-portrait-count-breakdown-v2"),
        selection_digest: sha256_json(&selected)?,
        entries: selected,
    })
}

#[derive(Default)]
struct ToolSample {
    value: String,
    original_bytes: usize,
    value_digest: String,
    count: usize,
}

fn build_tool_breakdown(eligible: &[&Item]) -> Result<CountBreakdown> {
    let mut total_occurrences = 0usize;
    let mut full = MultisetDigest::default();
    let mut samples: BTreeMap<String, ToolSample> = BTreeMap::new();

    // First pass retains only the 128 lowest deterministic value hashes. The
    // full multiset digest commits to every occurrence without a value map.
    for item in eligible {
        for_each_section_item(item.content(), "工具", |value| {
            total_occurrences += 1;
            let mut row = StableDigest::new("cognitive-portrait-tool-occurrence-v2");
            row.text(value);
            row.text(item.id().as_str());
            full.add(row.finish_bytes());
            let value_digest = sha256_bytes(value.as_bytes());
            if !samples.contains_key(&value_digest) {
                if samples.len() == MAX_BREAKDOWN_ENTRIES {
                    let largest = samples.last_key_value().map(|(key, _)| key.clone());
                    if largest.as_ref().is_some_and(|key| value_digest >= *key) {
                        return;
                    }
                    if let Some(largest) = largest {
                        samples.remove(&largest);
                    }
                }
                samples.insert(
                    value_digest.clone(),
                    ToolSample {
                        value: truncate_projection_text(value),
                        original_bytes: value.len(),
                        value_digest,
                        count: 0,
                    },
                );
            }
        });
    }

    // Second pass computes exact support only for retained values.
    for item in eligible {
        for_each_section_item(item.content(), "工具", |value| {
            let digest = sha256_bytes(value.as_bytes());
            if let Some(sample) = samples.get_mut(&digest) {
                sample.count += 1;
            }
        });
    }
    let mut entries: Vec<CountEntry> = samples
        .into_values()
        .filter(|sample| sample.count > 0)
        .map(|sample| CountEntry {
            value: sample.value,
            original_bytes: sample.original_bytes,
            value_digest: sample.value_digest,
            count: sample.count,
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.value.cmp(&right.value))
            .then_with(|| left.value_digest.cmp(&right.value_digest))
    });
    let selected_occurrences = entries.iter().map(|entry| entry.count).sum();
    Ok(CountBreakdown {
        total_occurrences,
        selected_occurrences,
        omitted_occurrences: total_occurrences - selected_occurrences,
        selected_entries: entries.len(),
        full_digest: full.finish("cognitive-portrait-tool-breakdown-v2"),
        selection_digest: sha256_json(&entries)?,
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

pub(super) fn for_each_section_item<'a>(
    content: &'a str,
    section: &str,
    mut visit: impl FnMut(&'a str),
) {
    let mut in_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.starts_with('-') {
            in_section = trimmed.trim_end_matches(':') == section;
        } else if in_section && trimmed.starts_with("- ") {
            visit(trimmed.trim_start_matches("- "));
        } else if in_section && !trimmed.is_empty() {
            in_section = false;
        }
    }
}

fn has_section_item(content: &str, section: &str) -> bool {
    let mut found = false;
    for_each_section_item(content, section, |_| found = true);
    found
}

pub(super) fn selection_digest(
    evidence: &[EvidenceRecord],
    dimensions: &PortraitDimensions,
    strata: &[super::bundle::SelectionStratum],
    evidence_byte_budget: usize,
) -> Result<String> {
    sha256_json(&(
        PORTRAIT_PROJECTION_POLICY,
        evidence_byte_budget as u64,
        evidence,
        dimensions,
        strata,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_digest_commits_to_declared_evidence_budget() {
        let dimensions = PortraitDimensions::default();
        let first = selection_digest(&[], &dimensions, &[], 1024).unwrap();
        let second = selection_digest(&[], &dimensions, &[], 2048).unwrap();
        assert_ne!(first, second);
    }
}
