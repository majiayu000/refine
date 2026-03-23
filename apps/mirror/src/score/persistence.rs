use anyhow::{Context, Result};
use std::io::{BufRead, Write};
use std::path::Path;

use crate::config::{ensure_mirror_dir, mirror_dir};

use super::types::ScoreResult;

// ── Persistence ──

pub fn persist_score(result: &ScoreResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("scores.jsonl");
    let line = serde_json::to_string(result)?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", line)?;
    drop(file);

    // Rotate: keep last 365 entries
    let content = std::fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 365 {
        let keep = &lines[lines.len() - 365..];
        std::fs::write(&path, keep.join("\n") + "\n")?;
    }
    Ok(())
}

pub fn load_recent_scores(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    load_recent_scores_from_path(&path, n)
}

pub(super) fn load_recent_scores_from_path(path: &Path, n: usize) -> Result<Vec<ScoreResult>> {
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
        let parsed = serde_json::from_str::<ScoreResult>(line).with_context(|| {
            format!(
                "failed to parse JSON on line {} in score history {}",
                line_no,
                path.display()
            )
        })?;
        all.push(parsed);
    }
    let start = all.len().saturating_sub(n);
    Ok(all[start..].to_vec())
}
