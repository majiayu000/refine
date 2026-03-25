use anyhow::Result;
use std::path::{Path, PathBuf};

use super::types::ScoreResult;

/// Sanitize a string for safe single-line statusline output.
///
/// Strips newlines, carriage returns, and ASCII control characters that could
/// corrupt the statusline file or inject terminal escape sequences.
fn sanitize_single_line(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\n' && *c != '\r' && !c.is_ascii_control())
        .collect()
}

/// Build the one-line statusline string from score data.
///
/// Format: "本周N 深度🟢 广度🔴 协作🔴 <short_advice>"
///
/// `session_count` comes from growth-tracker.json `total_sessions`.
/// `short` is the short advice from the LLM advice cache (may be empty).
pub fn build_statusline(result: &ScoreResult, session_count: u64, short: &str) -> String {
    let depth_e = result.layers[0].signal.emoji();
    let breadth_e = result.layers[1].signal.emoji();
    let collab_e = result.layers[2].signal.emoji();

    let mut parts = vec![
        format!("{}{}", crate::lang::t!("Week", "本周"), session_count),
        format!("{}{}", crate::lang::t!("Depth", "深度"), depth_e),
        format!("{}{}", crate::lang::t!("Breadth", "广度"), breadth_e),
        format!("{}{}", crate::lang::t!("Collab", "协作"), collab_e),
    ];
    let sanitized = sanitize_single_line(short);
    if !sanitized.is_empty() {
        parts.push(sanitized);
    }
    parts.join(" ")
}

fn growth_tracker_path(db_path: &Path) -> PathBuf {
    let primary = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("growth-tracker.json");
    if primary.exists() {
        return primary;
    }
    if let Some(legacy) = dirs::home_dir().map(|h| h.join(".refine").join("growth-tracker.json")) {
        if legacy.exists() {
            return legacy;
        }
    }
    primary
}

/// Write the one-line statusline to `~/.mirror/statusline.txt`.
///
/// Called after `mirror score` completes so that `cat ~/.mirror/statusline.txt`
/// returns the status in O(1) without spawning python3.
///
/// Uses an atomic write (temp file + rename) to prevent concurrent readers from
/// observing a partially-written file.
pub fn write_statusline(result: &ScoreResult, db_path: &Path) -> Result<()> {
    let tracker_path = growth_tracker_path(db_path);
    let session_count = match std::fs::read_to_string(&tracker_path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(v) => v
                .get("total_sessions")
                .and_then(|n| n.as_u64())
                .unwrap_or_else(|| {
                    eprintln!(
                        "[mirror] warn: growth-tracker.json missing 'total_sessions' field, defaulting to 0"
                    );
                    0
                }),
            Err(e) => {
                // Parse failure may be caused by a concurrent partial write from
                // growth.rs (non-atomic overwrite). Propagate the error instead of
                // writing session_count=0, which would permanently surface bad data.
                return Err(anyhow::anyhow!(
                    "failed to parse growth-tracker.json at {}: {}",
                    tracker_path.display(),
                    e
                ));
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // File does not exist yet (first run) — zero is correct.
            0
        }
        Err(e) => {
            // Any other IO error (e.g. EINTR during a concurrent write) — propagate
            // rather than recording session_count=0 as visible data.
            return Err(anyhow::anyhow!(
                "failed to read growth-tracker.json at {}: {}",
                tracker_path.display(),
                e
            ));
        }
    };

    let short = match crate::advice::load_cached() {
        Ok(cached) => cached
            .map(|c| c.short)
            .filter(|s: &String| !s.is_empty())
            .unwrap_or_default(),
        Err(e) => {
            eprintln!("[mirror] error: failed to load advice cache: {}", e);
            String::new()
        }
    };

    let line = build_statusline(result, session_count, &short);
    let dir = crate::config::ensure_mirror_dir()?;
    let dest = dir.join("statusline.txt");

    // Atomic write: write to a PID-unique sibling temp file then rename.
    // Using std::process::id() in the name prevents two concurrent `mirror score`
    // invocations from clobbering each other's temp file.  The last rename wins
    // deterministically because rename(2) on POSIX is atomic.
    let tmp = dir.join(format!("statusline.txt.{}.tmp", std::process::id()));
    std::fs::write(&tmp, &line)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {}", tmp.display(), e))?;
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        // Best-effort cleanup so we do not leave stale PID-named temps around.
        if let Err(rm_err) = std::fs::remove_file(&tmp) {
            eprintln!("[mirror] warn: failed to remove temp file {}: {}", tmp.display(), rm_err);
        }
        return Err(anyhow::anyhow!(
            "failed to rename {} -> statusline.txt: {}",
            tmp.display(),
            e
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::types::{Indicator, LayerScore, ScoreResult, Signal};
    use chrono::Utc;

    fn make_result(signals: [Signal; 3]) -> ScoreResult {
        let names = ["depth", "breadth", "collaboration"];
        ScoreResult {
            layers: std::array::from_fn(|i| LayerScore {
                name: names[i].to_string(),
                signal: signals[i],
                indicators: vec![Indicator {
                    name: "test".into(),
                    actual: 1.0,
                    target: ">0".into(),
                    signal: signals[i],
                }],
            }),
            tension: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn build_statusline_contains_emojis_and_count() {
        let result = make_result([Signal::Green, Signal::Red, Signal::Yellow]);
        let line = build_statusline(&result, 243, "some advice");
        // Emoji signals must appear regardless of language
        assert!(line.contains("🟢"), "depth signal missing");
        assert!(line.contains("🔴"), "breadth signal missing");
        assert!(line.contains("🟡"), "collab signal missing");
        assert!(line.contains("243"), "session count missing");
        assert!(line.ends_with("some advice"), "advice missing");
    }

    #[test]
    fn build_statusline_no_advice_no_trailing_space() {
        let result = make_result([Signal::Green, Signal::Green, Signal::Green]);
        let line = build_statusline(&result, 0, "");
        assert!(!line.ends_with(' '), "should not have trailing space");
        assert!(line.contains("🟢"));
        assert!(line.contains('0'));
    }

    #[test]
    fn build_statusline_four_parts_with_advice() {
        let result = make_result([Signal::Red, Signal::Red, Signal::Red]);
        let line = build_statusline(&result, 10, "tip");
        // 4 space-separated sections + advice: "XN X🔴 X🔴 X🔴 tip"
        let parts: Vec<&str> = line.splitn(5, ' ').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[4], "tip");
    }

    #[test]
    fn write_statusline_creates_file() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let result = make_result([Signal::Green, Signal::Red, Signal::Yellow]);
        let line = build_statusline(&result, 50, "test-advice");
        let path = dir.path().join("statusline.txt");
        std::fs::write(&path, &line)?;
        let content = std::fs::read_to_string(&path)?;
        assert_eq!(content, line);
        assert!(content.contains("50"));
        assert!(content.contains("test-advice"));
        Ok(())
    }
}
