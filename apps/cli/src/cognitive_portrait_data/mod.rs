//! Deterministic data collection and quality gates for cognitive portraits.
//!
//! This module performs no LLM work. It projects the same source-aware
//! snapshot and cohort used by Session Insights into a versioned evidence
//! bundle, then validates a candidate before archival.

mod bundle;
mod quality;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use refine_core::knowledge::ItemRepository;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

pub(crate) const MAX_PORTRAIT_BUNDLE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_PORTRAIT_CANDIDATE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_PREVIOUS_PORTRAIT_BYTES: usize = 4 * 1024 * 1024;

fn read_utf8_bounded(path: &Path, maximum: usize, label: &str) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if !metadata.file_type().is_file() {
        bail!("{label} must be a regular file: {}", path.display());
    }
    if metadata.len() > maximum as u64 {
        bail!(
            "{label} exceeds the {} byte limit: {}",
            maximum,
            path.display()
        );
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(path)
        .with_context(|| format!("open {label} {}", path.display()))?
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {label} {}", path.display()))?;
    if bytes.len() > maximum {
        bail!(
            "{label} exceeds the {} byte limit while reading: {}",
            maximum,
            path.display()
        );
    }
    String::from_utf8(bytes).with_context(|| format!("{label} is not UTF-8: {}", path.display()))
}

#[allow(unused_imports)]
pub(crate) use bundle::{
    build_bundle_from_snapshot, collect_bundle, read_bundle, write_bundle, CognitivePortraitBundle,
    PORTRAIT_BUNDLE_SCHEMA_VERSION, PORTRAIT_COLLECTOR_VERSION,
};
#[allow(unused_imports)]
pub(crate) use quality::{validate_portrait, PortraitQualityReport, PORTRAIT_QUALITY_GATE_VERSION};

pub(crate) async fn collect_to_file(
    item_store: &dyn ItemRepository,
    cutoff: Option<&str>,
    period_days: usize,
    output: &Path,
) -> Result<()> {
    let cutoff = cutoff
        .map(|raw| {
            DateTime::parse_from_rfc3339(raw)
                .map(|value| value.with_timezone(&Utc))
                .with_context(|| format!("--cutoff must be RFC3339, got {raw:?}"))
        })
        .transpose()?
        .unwrap_or_else(Utc::now);
    let bundle = collect_bundle(item_store, cutoff, period_days).await?;
    write_bundle(output, &bundle)?;
    println!(
        "{}",
        serde_json::json!({
            "output": output,
            "schema_version": bundle.schema_version,
            "collector_version": bundle.collector_version,
            "cutoff": bundle.cutoff,
            "comparison_status": bundle.comparison.status,
            "comparison_reasons": bundle.comparison.reasons,
        })
    );
    Ok(())
}

pub(crate) fn validate_files(
    bundle_path: &Path,
    portrait_path: &Path,
    previous_path: Option<&Path>,
    output: &Path,
) -> Result<()> {
    let bundle = read_bundle(bundle_path)?;
    let portrait = read_utf8_bounded(
        portrait_path,
        MAX_PORTRAIT_CANDIDATE_BYTES,
        "portrait candidate",
    )?;
    let previous = previous_path
        .map(|path| read_utf8_bounded(path, MAX_PREVIOUS_PORTRAIT_BYTES, "previous portrait"))
        .transpose()?;
    let report = validate_portrait(&bundle, &portrait, previous.as_deref());
    quality::write_quality_report(output, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !report.passed {
        bail!("QUALITY_GATE_FAILED: {}", report.errors.join("; "));
    }
    Ok(())
}
