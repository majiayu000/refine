use super::types::ScoreResult;
use crate::config::{ensure_mirror_dir, mirror_dir};
use anyhow::Result;
use std::io::{BufRead, Write};

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
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Ok(Vec::new()),
    };
    let reader = std::io::BufReader::new(file);
    let all: Vec<ScoreResult> = reader
        .lines()
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    let start = all.len().saturating_sub(n);
    Ok(all[start..].to_vec())
}
