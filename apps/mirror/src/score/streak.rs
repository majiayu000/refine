use chrono::{NaiveDate, Utc};

use super::types::ScoreResult;

/// Calculate consecutive days with score records, counting backwards from today.
///
/// Pure function: takes a slice of scores and the "today" date, returns the streak count.
/// Each day only needs at least one record to count. Days are derived from UTC timestamps.
pub fn calculate_streak(scores: &[ScoreResult], today: NaiveDate) -> u32 {
    if scores.is_empty() {
        return 0;
    }

    // Collect unique dates from scores
    let mut dates: Vec<NaiveDate> = scores.iter().map(|s| s.timestamp.date_naive()).collect();
    dates.sort_unstable();
    dates.dedup();

    // Count backwards from today
    let mut streak: u32 = 0;
    let mut expected = today;

    for &date in dates.iter().rev() {
        if date == expected {
            streak += 1;
            expected -= chrono::Duration::days(1);
        } else if date < expected {
            // Gap found — stop counting
            break;
        }
        // date > expected: future date, skip
    }

    streak
}

/// Return a milestone message if the streak hits a milestone, or None.
pub fn milestone_message(streak: u32) -> Option<&'static str> {
    match streak {
        365 => Some("🎊 整整一年！"),
        100 => Some("💯 百日里程碑！"),
        30 => Some("🏆 连续一个月！认知追踪已成习惯"),
        7 => Some("🎯 一周连续！习惯正在形成"),
        _ => None,
    }
}

/// Format streak for display. Returns None if streak < 2.
pub fn format_streak(streak: u32) -> Option<String> {
    if streak >= 2 {
        Some(format!("🔥 连续 {} 天", streak))
    } else {
        None
    }
}

/// Load scores and compute current streak. Convenience wrapper for integration.
pub fn current_streak() -> u32 {
    let scores = match super::persistence::load_score_activity(365) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let today = Utc::now().date_naive();
    calculate_streak(&scores, today)
}
