use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::{ensure_mirror_dir, mirror_dir};

use super::types::ScoreResult;

// ── Persistence ──

const SCORE_HISTORY_LIMIT: usize = 365;
pub(super) const SCORE_SCHEMA_VERSION: u32 = 3;

#[derive(Serialize)]
struct CurrentScore<'a> {
    score_schema_version: u32,
    #[serde(flatten)]
    score: &'a ScoreResult,
}

#[derive(Deserialize)]
struct ScoreSchemaEnvelope {
    #[serde(default)]
    score_schema_version: Option<u32>,
}

#[derive(Deserialize)]
struct ScoreActivity {
    #[serde(default = "legacy_score_timestamp")]
    timestamp: chrono::DateTime<chrono::Utc>,
}

struct HistoryLine {
    number: usize,
    json: String,
}

fn legacy_score_timestamp() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
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
    let history = match std::fs::read_to_string(path) {
        Ok(content) => history_lines_from_content(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read score history {}: {}",
                path.display(),
                e
            ));
        }
    };
    validate_history_before_append(path, &history)?;
    let mut lines: Vec<_> = history.into_iter().map(|line| line.json).collect();

    lines.push(serde_json::to_string(&CurrentScore {
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
    let mut compatible = Vec::new();
    for line in load_score_history_lines(path)? {
        match score_schema_version(&line.json, line.number, path)? {
            None | Some(0..=2) => {}
            Some(SCORE_SCHEMA_VERSION) => {
                compatible.push(parse_history_line::<ScoreResult>(
                    &line.json,
                    line.number,
                    path,
                )?);
            }
            Some(version) => return Err(unsupported_schema_error(version, path)),
        }
    }
    let start = compatible.len().saturating_sub(n);
    Ok(compatible[start..].to_vec())
}

pub(super) fn load_score_activity(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_score_activity_from_path(&path, n)
}

pub(super) fn load_score_activity_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
    let mut all = Vec::new();
    for line in load_score_history_lines(path)? {
        let activity = parse_history_line::<ScoreActivity>(&line.json, line.number, path)?;
        all.push(ScoreResult {
            timestamp: activity.timestamp,
            ..ScoreResult::default()
        });
    }
    let start = all.len().saturating_sub(n);
    Ok(all[start..].to_vec())
}

fn validate_history_before_append(path: &Path, lines: &[HistoryLine]) -> Result<()> {
    for line in lines {
        if let Some(version) = score_schema_version(&line.json, line.number, path)? {
            if version > SCORE_SCHEMA_VERSION {
                return Err(unsupported_schema_error(version, path));
            }
        }
    }
    Ok(())
}

fn score_schema_version(line: &str, line_no: usize, path: &Path) -> Result<Option<u32>> {
    Ok(parse_history_line::<ScoreSchemaEnvelope>(line, line_no, path)?.score_schema_version)
}

fn unsupported_schema_error(version: u32, path: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "failed to load metric history from score history {}: unsupported score schema version {}; current version is {}",
        path.display(),
        version,
        SCORE_SCHEMA_VERSION
    )
}

fn parse_history_line<T: for<'de> Deserialize<'de>>(
    line: &str,
    line_no: usize,
    path: &Path,
) -> Result<T> {
    serde_json::from_str::<T>(line).with_context(|| {
        format!(
            "failed to parse JSON on line {} in score history {}",
            line_no,
            path.display()
        )
    })
}

fn load_score_history_lines(path: &Path) -> Result<Vec<HistoryLine>> {
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
        all.push(HistoryLine {
            number: line_no,
            json: line.to_owned(),
        });
    }
    Ok(all)
}

fn history_lines_from_content(content: &str) -> Vec<HistoryLine> {
    content
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let json = line.trim();
            (!json.is_empty()).then(|| HistoryLine {
                number: idx + 1,
                json: json.to_owned(),
            })
        })
        .collect()
}
