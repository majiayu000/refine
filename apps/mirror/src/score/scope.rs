//! Reproducible scope for comparable daily scores. Display-only queries do not
//! receive a canonical scope and cannot enter personal history.
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::session::DataQualityStats;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::config::Targets;

const CANONICAL_WINDOW: &str = "rolling-90d";
const METHOD_VERSION: &str =
    "mirror-v6;session-start;linked-interactive-session-sources-v1;reviewed-curation-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreScope {
    pub database_identity: String,
    pub window: String,
    pub method: String,
    pub targets: String,
    pub window_start: DateTime<Utc>,
    pub window_end: DateTime<Utc>,
    pub cohort_identity: String,
    pub data_quality: DataQualityStats,
}

impl ScoreScope {
    pub fn canonical(
        database: &Path,
        targets: &Targets,
        cutoff: DateTime<Utc>,
        quality: &DataQualityStats,
    ) -> Result<Self> {
        let path = std::fs::canonicalize(database)
            .or_else(|_| std::path::absolute(database))
            .context("resolve Mirror database identity")?;
        // A canonical path makes aliases converge and keeps different databases
        // isolated. Moving a database intentionally starts a separate baseline.
        Ok(Self {
            database_identity: path.to_string_lossy().into_owned(),
            window: CANONICAL_WINDOW.into(),
            method: METHOD_VERSION.into(),
            targets: serde_json::to_string(&serde_json::to_value(targets)?)?,
            window_start: cutoff - Duration::days(90),
            window_end: cutoff,
            cohort_identity: quality.cohort_identity.clone(),
            data_quality: quality.clone(),
        })
    }

    pub fn is_canonical(&self) -> bool {
        self.window == CANONICAL_WINDOW
            && self.method == METHOD_VERSION
            && self.window_end - self.window_start == Duration::days(90)
            && !self.database_identity.is_empty()
            && !self.targets.is_empty()
    }

    pub fn compatible_with(&self, other: &Self) -> bool {
        self.is_canonical()
            && other.is_canonical()
            && self.database_identity == other.database_identity
            && self.window == other.window
            && self.method == other.method
            && self.targets == other.targets
    }

    pub(super) fn retention_key(&self) -> (&str, &str, &str, &str) {
        (
            &self.database_identity,
            &self.window,
            &self.method,
            &self.targets,
        )
    }
}
