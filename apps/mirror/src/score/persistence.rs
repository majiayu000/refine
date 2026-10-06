use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::mirror_dir;

use super::scope::ScoreScope;
use super::types::ScoreResult;

// ── Persistence ──

const SCORE_HISTORY_LIMIT: usize = 365;
pub(super) const SCORE_SCHEMA_VERSION: u32 = 6;

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

pub(super) fn persist_score_to_path(path: &Path, result: &ScoreResult) -> Result<bool> {
    let scope = result.scope.as_ref().ok_or_else(|| {
        anyhow::anyhow!("only a canonical rolling-90-day score can enter metric history")
    })?;
    if !scope.is_canonical() || result.timestamp != scope.window_end {
        anyhow::bail!("invalid canonical score scope or timestamp; refusing to persist score");
    }
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

    let published = write_result?;
    unlock_result?;
    Ok(published)
}

fn persist_score_to_path_locked(path: &Path, result: &ScoreResult) -> Result<bool> {
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
    // Group by comparable scope and UTC date. A slower concurrent writer must
    // not replace a newer snapshot from the same date.
    type ScopeKey = (String, String, String, String);
    type DailyRows = BTreeMap<chrono::NaiveDate, (chrono::DateTime<chrono::Utc>, String)>;
    let mut scoped: BTreeMap<ScopeKey, DailyRows> = BTreeMap::new();
    let mut legacy = Vec::new();
    for line in history {
        let timestamp =
            parse_history_line::<ScoreActivity>(&line.json, line.number, path)?.timestamp;
        let current =
            if score_schema_version(&line.json, line.number, path)? == Some(SCORE_SCHEMA_VERSION) {
                Some(parse_history_line::<ScoreResult>(
                    &line.json,
                    line.number,
                    path,
                )?)
            } else {
                None
            };
        if let Some(scope) = current
            .as_ref()
            .and_then(|score| score.scope.as_ref())
            .filter(|scope| scope.is_canonical() && scope.window_end == timestamp)
        {
            let (db, window, method, targets) = scope.retention_key();
            let daily = scoped
                .entry((db.into(), window.into(), method.into(), targets.into()))
                .or_default();
            let day = timestamp.date_naive();
            if daily
                .get(&day)
                .is_none_or(|(existing, _)| timestamp > *existing)
            {
                daily.insert(day, (timestamp, line.json));
            }
        } else {
            // Old and unscoped scores remain readable as activity only.
            legacy.push((timestamp, line.json));
        }
    }
    let scope = result.scope.as_ref().expect("validated canonical scope");
    let (db, window, method, targets) = scope.retention_key();
    let daily = scoped
        .entry((db.into(), window.into(), method.into(), targets.into()))
        .or_default();
    let day = result.timestamp.date_naive();
    let published = daily
        .get(&day)
        .is_none_or(|(existing, _)| result.timestamp > *existing);
    if published {
        daily.insert(
            day,
            (
                result.timestamp,
                serde_json::to_string(&CurrentScore {
                    score_schema_version: SCORE_SCHEMA_VERSION,
                    score: result,
                })?,
            ),
        );
    }
    legacy.sort_by_key(|(timestamp, _)| *timestamp);
    let legacy_start = legacy.len().saturating_sub(SCORE_HISTORY_LIMIT);
    let mut retained = legacy.into_iter().skip(legacy_start).collect::<Vec<_>>();
    for daily in scoped.into_values() {
        let start = daily.len().saturating_sub(SCORE_HISTORY_LIMIT);
        retained.extend(daily.into_values().skip(start));
    }
    retained.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let lines = retained
        .into_iter()
        .map(|(_, json)| json)
        .collect::<Vec<_>>();

    write_lines_atomically(path, &lines)?;
    Ok(published)
}

pub(super) fn write_lines_atomically(path: &Path, lines: &[String]) -> Result<()> {
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

pub fn load_recent_scores(n: usize, scope: &ScoreScope) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_recent_scores_for_scope_from_path(&path, n, Some(scope))
}

#[cfg(test)]
pub(super) fn load_recent_scores_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
    load_recent_scores_for_scope_from_path(path, n, None)
}

pub(super) fn load_recent_scores_for_scope_from_path(
    path: &Path,
    n: usize,
    expected_scope: Option<&ScoreScope>,
) -> Result<Vec<ScoreResult>> {
    let mut compatible = Vec::new();
    for line in load_score_history_lines(path)? {
        match score_schema_version(&line.json, line.number, path)? {
            None | Some(0..=5) => {}
            Some(SCORE_SCHEMA_VERSION) => {
                let score = parse_history_line::<ScoreResult>(&line.json, line.number, path)?;
                let Some(scope) = score.scope.as_ref() else {
                    continue;
                };
                if !scope.is_canonical() || scope.window_end != score.timestamp {
                    continue;
                }
                if expected_scope.is_some_and(|expected| {
                    !scope.compatible_with(expected) || score.timestamp > expected.window_end
                }) {
                    continue;
                }
                compatible.push(score);
            }
            Some(version) => return Err(unsupported_schema_error(version, path)),
        }
    }
    compatible.sort_by_key(|score| score.timestamp);
    let start = compatible.len().saturating_sub(n);
    Ok(compatible.into_iter().skip(start).collect())
}

pub(super) fn load_score_activity(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_score_activity_from_path(&path, n)
}

pub(super) fn load_score_activity_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
    // Activity is a day-level habit across scopes. Multiple databases scored
    // on one day must not consume several days of the streak lookback.
    let now = chrono::Utc::now();
    let mut by_day = BTreeMap::new();
    for line in load_score_history_lines(path)? {
        let activity = parse_history_line::<ScoreActivity>(&line.json, line.number, path)?;
        if activity.timestamp > now {
            continue;
        }
        by_day
            .entry(activity.timestamp.date_naive())
            .and_modify(|timestamp: &mut chrono::DateTime<chrono::Utc>| {
                *timestamp = (*timestamp).max(activity.timestamp)
            })
            .or_insert(activity.timestamp);
    }
    let start = by_day.len().saturating_sub(n);
    Ok(by_day
        .into_values()
        .skip(start)
        .map(|timestamp| ScoreResult {
            timestamp,
            ..ScoreResult::default()
        })
        .collect())
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
