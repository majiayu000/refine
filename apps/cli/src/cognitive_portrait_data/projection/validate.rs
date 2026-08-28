use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use super::super::bundle::{
    CognitivePortraitBundle, CountBreakdown, DimensionProjection, FieldFingerprint,
    PortraitDimensions, PortraitWindowData, MAX_BREAKDOWN_ENTRIES, MAX_CLAIM_CATALOG_BYTES,
    MAX_DIMENSION_ENTRIES, MAX_DIMENSION_EVIDENCE_IDS, MAX_PROJECTED_BUNDLE_BYTES,
    MAX_PROJECTION_TEXT_BYTES, MAX_SELECTED_EVIDENCE_PER_WINDOW, MAX_WINDOW_DIMENSIONS_BYTES,
    MAX_WINDOW_EVIDENCE_BYTES, PORTRAIT_PROJECTION_POLICY,
};
use super::hashing::{sha256_bytes, valid_digest};
use super::{primary_category, selection_digest};

pub(crate) fn validate_window_projection(data: &PortraitWindowData, window: &str) -> Result<()> {
    let selection = &data.evidence_selection;
    if selection.policy_version != PORTRAIT_PROJECTION_POLICY {
        bail!("SCHEMA_INVALID: {window} projection policy is unsupported");
    }
    if selection.evidence_byte_budget != MAX_WINDOW_EVIDENCE_BYTES
        || selection.eligible_observations
            != selection.selected_observations + selection.omitted_observations
        || selection.selected_observations != data.evidence.len()
        || selection.selected_observations > MAX_SELECTED_EVIDENCE_PER_WINDOW
        || !valid_digest(&selection.full_payload_digest)
        || !valid_digest(&selection.selection_digest)
    {
        bail!("SCHEMA_INVALID: {window} evidence selection invariant failed");
    }

    let mut evidence_ids = BTreeSet::new();
    let mut evidence_times = BTreeMap::new();
    let mut previous_order = None;
    for record in &data.evidence {
        let order = (record.event_time, record.item_id.as_str());
        if previous_order.is_some_and(|previous| previous >= order)
            || !evidence_ids.insert(record.evidence_id.clone())
            || record.evidence_id != format!("obs:{}", record.item_id)
            || record.display_text.len() > MAX_PROJECTION_TEXT_BYTES
            || record.display_text_original_bytes < record.display_text.len()
            || !valid_digest(&record.display_text_digest)
            || (record.display_text_original_bytes == record.display_text.len()
                && record.display_text_digest != sha256_bytes(record.display_text.as_bytes()))
            || !valid_fingerprint(&record.original_fields.title)
            || !valid_fingerprint(&record.original_fields.summary)
            || !valid_fingerprint(&record.original_fields.content)
            || record
                .original_fields
                .excerpt
                .as_ref()
                .is_some_and(|value| !valid_fingerprint(value))
            || !valid_fingerprint(&record.original_fields.tags)
            || record.categories.is_empty()
            || !record.categories.windows(2).all(|pair| pair[0] < pair[1])
        {
            bail!("SCHEMA_INVALID: {window} evidence record invariant failed");
        }
        evidence_times.insert(record.evidence_id.clone(), record.event_time);
        previous_order = Some(order);
    }

    let mut stratum_keys = BTreeSet::new();
    let mut previous_stratum_key = None;
    let mut eligible_sum = 0usize;
    let mut selected_sum = 0usize;
    let mut omitted_sum = 0usize;
    for stratum in &selection.strata {
        let key = (
            stratum.source.as_str(),
            stratum.category.as_str(),
            stratum.project_bucket.as_str(),
        );
        if previous_stratum_key.is_some_and(|previous| previous >= key)
            || !stratum_keys.insert(key)
            || stratum.eligible_observations
                != stratum.selected_observations + stratum.omitted_observations
        {
            bail!("SCHEMA_INVALID: {window} stratum invariant failed");
        }
        eligible_sum += stratum.eligible_observations;
        selected_sum += stratum.selected_observations;
        omitted_sum += stratum.omitted_observations;
        previous_stratum_key = Some(key);
    }
    if (eligible_sum, selected_sum, omitted_sum)
        != (
            selection.eligible_observations,
            selection.selected_observations,
            selection.omitted_observations,
        )
    {
        bail!("SCHEMA_INVALID: {window} stratum totals disagree with selection");
    }
    validate_selected_strata(data, window)?;

    validate_count_breakdown(&data.metrics.project_ranking, window, "project_ranking")?;
    validate_count_breakdown(&data.metrics.tool_frequency, window, "tool_frequency")?;
    for (name, dimension) in dimensions(&data.dimensions) {
        validate_dimension(dimension, &evidence_ids, &evidence_times, window, name)?;
    }
    let expected_selection_digest = selection_digest(
        &data.evidence,
        &data.dimensions,
        &selection.strata,
        selection.evidence_byte_budget,
    )?;
    if selection.selection_digest != expected_selection_digest {
        bail!("SCHEMA_INVALID: {window} selection digest mismatch");
    }
    Ok(())
}

fn valid_fingerprint(value: &FieldFingerprint) -> bool {
    valid_digest(&value.digest)
}

fn validate_count_breakdown(breakdown: &CountBreakdown, window: &str, name: &str) -> Result<()> {
    if breakdown.total_entries != breakdown.selected_entries + breakdown.omitted_entries
        || breakdown.selected_entries != breakdown.entries.len()
        || breakdown.selected_entries > MAX_BREAKDOWN_ENTRIES
        || !valid_digest(&breakdown.full_digest)
        || !valid_digest(&breakdown.selection_digest)
    {
        bail!("SCHEMA_INVALID: {window} {name} count breakdown invariant failed");
    }
    for entry in &breakdown.entries {
        if entry.value.len() > MAX_PROJECTION_TEXT_BYTES
            || entry.original_bytes < entry.value.len()
            || !valid_digest(&entry.value_digest)
            || (entry.original_bytes == entry.value.len()
                && entry.value_digest != sha256_bytes(entry.value.as_bytes()))
        {
            bail!("SCHEMA_INVALID: {window} {name} entry invariant failed");
        }
    }
    if !breakdown.entries.windows(2).all(|pair| {
        pair[0].count > pair[1].count
            || (pair[0].count == pair[1].count && pair[0].value <= pair[1].value)
    }) {
        bail!("SCHEMA_INVALID: {window} {name} ordering invariant failed");
    }
    if breakdown.selection_digest != super::hashing::sha256_json(&breakdown.entries)? {
        bail!("SCHEMA_INVALID: {window} {name} selection digest mismatch");
    }
    Ok(())
}

fn dimensions(dimensions: &PortraitDimensions) -> [(&'static str, &DimensionProjection); 7] {
    [
        ("projects", &dimensions.projects),
        ("decisions", &dimensions.decisions),
        ("bugfixes", &dimensions.bugfixes),
        ("knowledge", &dimensions.knowledge),
        ("patterns", &dimensions.patterns),
        ("architectures", &dimensions.architectures),
        ("frictions", &dimensions.frictions),
    ]
}

fn validate_dimension(
    dimension: &DimensionProjection,
    evidence_ids: &BTreeSet<String>,
    evidence_times: &BTreeMap<String, DateTime<Utc>>,
    window: &str,
    name: &str,
) -> Result<()> {
    if dimension.total_occurrences != dimension.selected_occurrences + dimension.omitted_occurrences
        || dimension.selected_values != dimension.entries.len()
        || dimension.selected_values > MAX_DIMENSION_ENTRIES
        || !valid_digest(&dimension.full_digest)
    {
        bail!("SCHEMA_INVALID: {window} {name} dimension invariant failed");
    }
    let mut selected_refs = 0usize;
    for entry in &dimension.entries {
        if entry.value.len() > MAX_PROJECTION_TEXT_BYTES
            || entry.original_bytes < entry.value.len()
            || !valid_digest(&entry.value_digest)
            || (entry.original_bytes == entry.value.len()
                && entry.value_digest != sha256_bytes(entry.value.as_bytes()))
            || entry.support_count != entry.evidence_ids.len() + entry.omitted_evidence_count
            || entry.evidence_ids.is_empty()
            || entry.evidence_ids.len() > MAX_DIMENSION_EVIDENCE_IDS
        {
            bail!("SCHEMA_INVALID: {window} {name} entry invariant failed");
        }
        let unique: BTreeSet<&str> = entry.evidence_ids.iter().map(String::as_str).collect();
        if unique.len() != entry.evidence_ids.len()
            || entry
                .evidence_ids
                .iter()
                .any(|evidence_id| !evidence_ids.contains(evidence_id))
        {
            bail!("SCHEMA_INVALID: {window} {name} has a dangling evidence reference");
        }
        selected_refs += entry.evidence_ids.len();
        if !entry.evidence_ids.windows(2).all(|pair| {
            evidence_times[&pair[0]] > evidence_times[&pair[1]]
                || (evidence_times[&pair[0]] == evidence_times[&pair[1]] && pair[0] < pair[1])
        }) {
            bail!("SCHEMA_INVALID: {window} {name} evidence ordering is unstable");
        }
    }
    let selected_occurrences: usize = dimension
        .entries
        .iter()
        .map(|entry| entry.support_count)
        .sum();
    if selected_refs != dimension.selected_evidence_refs
        || selected_occurrences != dimension.selected_occurrences
    {
        bail!("SCHEMA_INVALID: {window} {name} reference totals disagree");
    }
    if !dimension.entries.windows(2).all(|pair| {
        pair[0].support_count > pair[1].support_count
            || (pair[0].support_count == pair[1].support_count
                && (evidence_times[&pair[0].evidence_ids[0]]
                    > evidence_times[&pair[1].evidence_ids[0]]
                    || (evidence_times[&pair[0].evidence_ids[0]]
                        == evidence_times[&pair[1].evidence_ids[0]]
                        && pair[0].value_digest <= pair[1].value_digest)))
    }) {
        bail!("SCHEMA_INVALID: {window} {name} dimension ordering is unstable");
    }
    Ok(())
}

fn validate_selected_strata(data: &PortraitWindowData, window: &str) -> Result<()> {
    let top_projects: BTreeSet<&str> = data
        .metrics
        .project_ranking
        .entries
        .iter()
        .take(super::super::bundle::MAX_TOP_PROJECT_STRATA)
        .map(|entry| entry.value.as_str())
        .collect();
    let mut actual: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for record in &data.evidence {
        let project_bucket = if top_projects.contains(record.project.as_str()) {
            record.project.clone()
        } else {
            "__other__".to_string()
        };
        *actual
            .entry((
                record.source.clone(),
                primary_category(&record.categories).to_string(),
                project_bucket,
            ))
            .or_default() += 1;
    }
    for stratum in &data.evidence_selection.strata {
        let key = (
            stratum.source.clone(),
            stratum.category.clone(),
            stratum.project_bucket.clone(),
        );
        if actual.remove(&key).unwrap_or(0) != stratum.selected_observations {
            bail!("SCHEMA_INVALID: {window} selected evidence disagrees with its stratum");
        }
    }
    if !actual.is_empty() {
        bail!("SCHEMA_INVALID: {window} selected evidence has an undisclosed stratum");
    }
    Ok(())
}

pub(crate) fn enforce_bundle_budgets(bundle: &CognitivePortraitBundle) -> Result<()> {
    for (window, data) in [("current", &bundle.current), ("previous", &bundle.previous)] {
        enforce_component_budget(
            window,
            "evidence",
            &data.evidence,
            MAX_WINDOW_EVIDENCE_BYTES,
        )?;
        enforce_component_budget(
            window,
            "dimensions",
            &data.dimensions,
            MAX_WINDOW_DIMENSIONS_BYTES,
        )?;
    }
    enforce_component_budget(
        "bundle",
        "claim_catalog",
        &bundle.claim_catalog,
        MAX_CLAIM_CATALOG_BYTES,
    )?;
    let bytes = serialized_bytes(bundle, true)
        .context("serialize cognitive portrait bundle for internal budget")?
        + 1;
    if bytes > MAX_PROJECTED_BUNDLE_BYTES {
        bail!(
            "INTERNAL_BUDGET_VIOLATION: projected bundle uses {bytes} bytes; limit is {MAX_PROJECTED_BUNDLE_BYTES}"
        );
    }
    Ok(())
}

fn enforce_component_budget<T: Serialize>(
    window: &str,
    component: &str,
    value: &T,
    limit: usize,
) -> Result<()> {
    let bytes = serialized_bytes(value, false)
        .with_context(|| format!("serialize {window} {component} for budget"))?;
    if bytes > limit {
        bail!(
            "INTERNAL_BUDGET_VIOLATION: {window} {component} uses {bytes} bytes; limit is {limit}"
        );
    }
    Ok(())
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::other("serialized byte count overflow"))?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn serialized_bytes<T: Serialize>(value: &T, pretty: bool) -> Result<usize> {
    let mut writer = CountingWriter::default();
    if pretty {
        serde_json::to_writer_pretty(&mut writer, value)?;
    } else {
        serde_json::to_writer(&mut writer, value)?;
    }
    Ok(writer.bytes)
}
