use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Green,
    Yellow,
    Red,
}

impl Signal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Signal::Green => "green",
            Signal::Yellow => "yellow",
            Signal::Red => "red",
        }
    }

    pub const fn emoji(self) -> &'static str {
        match self {
            Signal::Green => "🟢",
            Signal::Yellow => "🟡",
            Signal::Red => "🔴",
        }
    }
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
