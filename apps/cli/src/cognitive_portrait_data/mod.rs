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
use std::fs;
use std::path::Path;

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
    let portrait = fs::read_to_string(portrait_path)
        .with_context(|| format!("read portrait candidate {}", portrait_path.display()))?;
    let previous = previous_path
        .map(|path| {
            fs::read_to_string(path)
                .with_context(|| format!("read previous portrait {}", path.display()))
        })
        .transpose()?;
    let report = validate_portrait(&bundle, &portrait, previous.as_deref());
    quality::write_quality_report(output, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if !report.passed {
        bail!("QUALITY_GATE_FAILED: {}", report.errors.join("; "));
    }
    Ok(())
}
