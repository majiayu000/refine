//! Deterministic data collection and quality gates for cognitive portraits.
//!
//! This module performs no LLM work. It projects the same source-aware
//! snapshot and cohort used by Session Insights into a versioned evidence
//! bundle, then validates a candidate before archival.

mod bundle;
mod quality;

#[allow(unused_imports)]
pub(crate) use bundle::{
    build_bundle_from_snapshot, collect_bundle, read_bundle, write_bundle, CognitivePortraitBundle,
    PORTRAIT_BUNDLE_SCHEMA_VERSION, PORTRAIT_COLLECTOR_VERSION,
};
#[allow(unused_imports)]
pub(crate) use quality::{validate_portrait, PortraitQualityReport, PORTRAIT_QUALITY_GATE_VERSION};
