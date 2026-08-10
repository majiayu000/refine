use anyhow::Result;
use std::path::Path;

use super::baseline::PersonalTrends;
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
/// Format: "🪞🟡🔴🔴 🔥4天 <short_advice>"
///
/// `short` is the short advice from the LLM advice cache (may be empty).
pub fn build_statusline(
    result: &ScoreResult,
    short: &str,
    trends: Option<&PersonalTrends>,
) -> String {
    let depth_e = result.layers[0].signal.emoji();
    let breadth_e = result.layers[1].signal.emoji();
    let collab_e = result.layers[2].signal.emoji();
    let arrow = trends
        .and_then(PersonalTrends::overall)
        .map(|trend| trend.arrow())
        .unwrap_or("");

    let streak = super::streak::current_streak();

    let mut parts = vec![format!("🪞{}{}{}{}", depth_e, breadth_e, collab_e, arrow)];
    if streak >= 2 {
        parts.push(format!("🔥{}天", streak));
    }
    let sanitized = sanitize_single_line(short);
    if !sanitized.is_empty() {
        parts.push(sanitized);
    }
    parts.join(" ")
}

/// Write the one-line statusline to `~/.mirror/statusline.txt`.
///
/// Called after `mirror score` completes so that `cat ~/.mirror/statusline.txt`
/// returns the status in O(1) without spawning python3.
///
/// Uses an atomic write (temp file + rename) to prevent concurrent readers from
/// observing a partially-written file.
pub fn write_statusline(
    result: &ScoreResult,
    _db_path: &Path,
    trends: Option<&PersonalTrends>,
) -> Result<()> {
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

    let line = build_statusline(result, &short, trends);
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
            eprintln!(
                "[mirror] warn: failed to remove temp file {}: {}",
                tmp.display(),
                rm_err
            );
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
    fn build_statusline_compact_format() {
        let result = make_result([Signal::Green, Signal::Red, Signal::Yellow]);
        let line = build_statusline(&result, "some advice", None);
        assert!(line.starts_with("🪞"), "should start with mirror emoji");
        assert!(line.contains("🟢"), "depth signal missing");
        assert!(line.contains("🔴"), "breadth signal missing");
        assert!(line.contains("🟡"), "collab signal missing");
        assert!(line.ends_with("some advice"), "advice missing");
    }

    #[test]
    fn build_statusline_no_advice_no_trailing_space() {
        let result = make_result([Signal::Green, Signal::Green, Signal::Green]);
        let line = build_statusline(&result, "", None);
        assert!(!line.ends_with(' '), "should not have trailing space");
        assert!(line.starts_with("🪞🟢🟢🟢"));
    }

    #[test]
    fn build_statusline_ends_with_advice() {
        let result = make_result([Signal::Red, Signal::Red, Signal::Red]);
        let line = build_statusline(&result, "tip", None);
        assert!(line.ends_with("tip"), "advice should be last: {}", line);
        assert!(line.starts_with("🪞🔴🔴🔴"));
    }

    #[test]
    fn build_statusline_appends_one_personal_trend_arrow() {
        use crate::score::baseline::{compute_personal_trends, PersonalBaseline};

        let mut result = make_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let indicators = [
            ("dreyfus", 4.0),
            ("exploration", 20.0),
            ("bug_decision", 0.3),
        ];
        for (layer, (name, actual)) in result.layers.iter_mut().zip(indicators) {
            layer.indicators[0].name = name.into();
            layer.indicators[0].actual = actual;
        }
        let baseline = PersonalBaseline::from_averages(&[
            ("dreyfus", 3.0),
            ("exploration", 15.0),
            ("bug_decision", 0.6),
        ]);
        let trends = compute_personal_trends(&result, &baseline);
        let line = build_statusline(&result, "", Some(&trends));

        assert!(line.starts_with("🪞🟢🟡🔴↑"));
        assert_eq!(line.chars().filter(|c| "↑→↓".contains(*c)).count(), 1);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn write_statusline_creates_file() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let result = make_result([Signal::Green, Signal::Red, Signal::Yellow]);
        let line = build_statusline(&result, "test-advice", None);
        let path = dir.path().join("statusline.txt");
        std::fs::write(&path, &line)?;
        let content = std::fs::read_to_string(&path)?;
        assert_eq!(content, line);
        assert!(content.contains("test-advice"));
        Ok(())
    }
}
