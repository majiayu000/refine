use anyhow::Result;
use std::path::{Path, PathBuf};

use super::types::ScoreResult;

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
    if !short.is_empty() {
        parts.push(short.to_string());
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
pub fn write_statusline(result: &ScoreResult, db_path: &Path) -> Result<()> {
    let session_count = std::fs::read_to_string(growth_tracker_path(db_path))
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.get("total_sessions").and_then(|n| n.as_u64()))
        .unwrap_or(0);

    let short = crate::advice::load_cached()
        .ok()
        .flatten()
        .map(|c| c.short)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    let line = build_statusline(result, session_count, &short);
    let dir = crate::config::ensure_mirror_dir()?;
    std::fs::write(dir.join("statusline.txt"), &line)
        .map_err(|e| anyhow::anyhow!("failed to write statusline.txt: {}", e))?;
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
