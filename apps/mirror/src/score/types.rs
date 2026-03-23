use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const DECISION_KEYWORDS: &[&str] = &[
    "因为", "因", "原因", "选择", "采用", "because", "reason", "chose", "chosen", "adopted",
    "selected",
];

/// Minimum number of historical scores to activate personal baseline
pub const BASELINE_MIN_ENTRIES: usize = 7;

/// Sliding window size in days
pub const BASELINE_WINDOW_DAYS: i64 = 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Green,
    Yellow,
    Red,
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Signal::Green => write!(f, "\x1b[32m●\x1b[0m"),
            Signal::Yellow => write!(f, "\x1b[33m●\x1b[0m"),
            Signal::Red => write!(f, "\x1b[31m●\x1b[0m"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Indicator {
    pub name: String,
    pub actual: f64,
    pub target: String,
    pub signal: Signal,
}

impl Indicator {
    pub fn display_value(&self) -> String {
        match self.name.as_str() {
            "mode_diversity" => format!("{}", self.actual as usize),
            "bug_decision" => format!("{:.2}", self.actual),
            "dreyfus" => format!("{:.1}", self.actual),
            "knowledge_rate" | "friction_density" => format!("{:.1}", self.actual),
            _ => format!("{:.0}%", self.actual),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerScore {
    pub name: String,
    pub signal: Signal,
    pub indicators: Vec<Indicator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreResult {
    pub layers: [LayerScore; 3],
    pub tension: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// 4-week rolling averages for each indicator
#[derive(Debug, Clone)]
pub struct PersonalBaseline {
    pub dreyfus_avg: f64,
    pub decision_quality_avg: f64,
    pub depth_output_avg: f64,
    pub exploration_avg: f64,
    pub deep_invest_avg: f64,
    pub fragmentation_avg: f64,
    pub delegation_avg: f64,
    pub mode_diversity_avg: f64,
    pub bug_decision_avg: f64,
    pub knowledge_rate_avg: f64,
    pub friction_density_avg: f64,
}

/// Green > Yellow > Red
pub(super) fn worst(signals: &[Signal]) -> Signal {
    if signals.contains(&Signal::Red) {
        Signal::Red
    } else if signals.contains(&Signal::Yellow) {
        Signal::Yellow
    } else {
        Signal::Green
    }
}
