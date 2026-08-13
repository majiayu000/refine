use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::{ensure_mirror_dir, mirror_dir};

use super::indicators::{canonical_indicator_key, indicator_specs};
use super::types::ScoreResult;

// ── Persistence ──

const SCORE_HISTORY_LIMIT: usize = 365;
pub(super) const SCORE_SCHEMA_VERSION: u32 = 2;

#[derive(Serialize)]
struct PersistedScoreRef<'a> {
    score_schema_version: u32,
    #[serde(flatten)]
    score: &'a ScoreResult,
}

#[derive(Deserialize)]
struct PersistedScore {
    #[serde(default)]
    score_schema_version: Option<u32>,
    #[serde(flatten)]
    score: ScoreResult,
}

impl PersistedScore {
    fn uses_current_scoring_semantics(&self) -> bool {
        match self.score_schema_version {
            Some(version) => version == SCORE_SCHEMA_VERSION,
            // PR #146 briefly produced unversioned snapshots with the current
            // eight-indicator contract. Accept only that exact fingerprint;
            // older unversioned snapshots used incompatible breadth formulas.
            None => has_current_indicator_contract(&self.score),
        }
    }
}

fn has_current_indicator_contract(score: &ScoreResult) -> bool {
    let mut actual: Vec<&str> = score
        .layers
        .iter()
        .flat_map(|layer| &layer.indicators)
        .map(|indicator| canonical_indicator_key(&indicator.name))
        .collect();
    actual.sort_unstable();
    actual.dedup();

    let mut expected: Vec<&str> = indicator_specs().iter().map(|spec| spec.key).collect();
    expected.sort_unstable();
    actual == expected
}

pub fn persist_score(result: &ScoreResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("scores.jsonl");
    persist_score_to_path(&path, result)
}

pub(super) fn persist_score_to_path(path: &Path, result: &ScoreResult) -> Result<()> {
    let lock_path = path.with_extension("lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open score lock {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("failed to acquire score lock {}", lock_path.display()))?;

    let write_result = persist_score_to_path_locked(path, result);
    let unlock_result = fs2::FileExt::unlock(&lock_file)
        .with_context(|| format!("failed to release score lock {}", lock_path.display()));

    write_result?;
    unlock_result?;
    Ok(())
}

fn persist_score_to_path_locked(path: &Path, result: &ScoreResult) -> Result<()> {
    let mut lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read score history {}: {}",
                path.display(),
                e
            ));
        }
    };

    lines.push(serde_json::to_string(&PersistedScoreRef {
        score_schema_version: SCORE_SCHEMA_VERSION,
        score: result,
    })?);
    if lines.len() > SCORE_HISTORY_LIMIT {
        let trim = lines.len() - SCORE_HISTORY_LIMIT;
        lines.drain(0..trim);
    }

    write_lines_atomically(path, &lines)
}

fn write_lines_atomically(path: &Path, lines: &[String]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "score history path has no parent directory: {}",
            path.display()
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("scores.jsonl");
    let nonce = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let temp_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        nonce
    ));

    let write_result = (|| -> Result<()> {
        let mut temp_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create temp score file {}", temp_path.display()))?;

        for line in lines {
            writeln!(temp_file, "{}", line)?;
        }
        temp_file
            .sync_all()
            .with_context(|| format!("failed to fsync temp score file {}", temp_path.display()))?;

        std::fs::rename(&temp_path, path).with_context(|| {
            format!(
                "failed to atomically replace score history {}",
                path.display()
            )
        })?;

        if let Ok(dir_file) = std::fs::File::open(parent) {
            let _ = dir_file.sync_all();
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    write_result
}

pub fn load_recent_scores(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_recent_scores_from_path(&path, n)
}

pub(super) fn load_recent_scores_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
    let all = load_persisted_scores_from_path(path)?;
    let compatible: Vec<ScoreResult> = all
        .into_iter()
        .filter(PersistedScore::uses_current_scoring_semantics)
        .map(|entry| entry.score)
        .collect();
    let start = compatible.len().saturating_sub(n);
    Ok(compatible[start..].to_vec())
}

/// Load score timestamps across every historical scoring schema.
///
/// Streaks track whether scoring ran on a day, so a scoring-formula migration
/// must not reset them. Metric trends use `load_recent_scores` instead and are
/// intentionally restricted to comparable score schemas.
pub(super) fn load_score_activity(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_score_activity_from_path(&path, n)
}

pub(super) fn load_score_activity_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
    let all: Vec<ScoreResult> = load_persisted_scores_from_path(path)?
        .into_iter()
        .map(|entry| entry.score)
        .collect();
    let start = all.len().saturating_sub(n);
    Ok(all[start..].to_vec())
}

fn load_persisted_scores_from_path(path: &Path) -> Result<Vec<PersistedScore>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to open score history {}: {}",
                path.display(),
                e
            ));
        }
    };
    let reader = std::io::BufReader::new(file);
    let mut all = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read line {} from score history {}",
                line_no,
                path.display()
            )
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<PersistedScore>(line).with_context(|| {
            format!(
                "failed to parse JSON on line {} in score history {}",
                line_no,
                path.display()
            )
        })?;
        all.push(parsed);
    }
    Ok(all)
}
