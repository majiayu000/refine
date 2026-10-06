use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;

use super::indicators::format_indicator_value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Green,
    Yellow,
    Red,
    /// The metric has no usable evidence. This is not a behavioral rating.
    Unknown,
}

impl Signal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Signal::Green => "green",
            Signal::Yellow => "yellow",
            Signal::Red => "red",
            Signal::Unknown => "unknown",
        }
    }

    pub const fn emoji(self) -> &'static str {
        match self {
            Signal::Green => "🟢",
            Signal::Yellow => "🟡",
            Signal::Red => "🔴",
            Signal::Unknown => "⚪",
        }
    }

    pub const fn ansi_dot(self) -> &'static str {
        match self {
            Signal::Green => "\x1b[32m●\x1b[0m",
            Signal::Yellow => "\x1b[33m●\x1b[0m",
            Signal::Red => "\x1b[31m●\x1b[0m",
            Signal::Unknown => "\x1b[90m●\x1b[0m",
        }
    }

    pub const fn plain_dot(self) -> &'static str {
        self.emoji()
    }

    pub const fn render(self, ansi: bool) -> &'static str {
        if ansi {
            self.ansi_dot()
        } else {
            self.plain_dot()
        }
    }

    fn supports_ansi_on_stdout() -> bool {
        std::io::stdout().is_terminal()
    }
}

impl std::fmt::Display for Signal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.render(Self::supports_ansi_on_stdout()))
    }
}

/// Direction relative to the user's rolling 28-day average. `Up` always means
/// improvement after accounting for whether lower or higher values are better.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trend {
    Up,
    Flat,
    Down,
}

impl Trend {
    pub const fn arrow(self) -> &'static str {
        match self {
            Trend::Up => "↑",
            Trend::Flat => "→",
            Trend::Down => "↓",
        }
    }
}

/// The population actually observed by a metric, independently of its value.
/// A zero value with observed evidence differs from an absent measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCoverage {
    pub observed: usize,
    pub eligible: usize,
    pub unit: String,
}

impl EvidenceCoverage {
    pub fn new(observed: usize, eligible: usize, unit: &str) -> Self {
        Self {
            observed,
            eligible,
            unit: unit.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Indicator {
    pub name: String,
    /// None serializes as null; old numeric values deserialize as Some(value).
    pub actual: Option<f64>,
    pub target: String,
    pub signal: Signal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<EvidenceCoverage>,
}

impl Indicator {
    pub fn display_value(&self) -> String {
        self.observed_value()
            .map(|actual| format_indicator_value(&self.name, actual))
            .unwrap_or_else(|| crate::lang::t!("n/a", "证据不足").to_string())
    }

    pub fn observed_value(&self) -> Option<f64> {
        self.actual
            .filter(|value| self.signal != Signal::Unknown && value.is_finite())
    }

    pub fn coverage_label(&self) -> String {
        let Some(coverage) = &self.coverage else {
            return String::new();
        };
        let unit = match coverage.unit.as_str() {
            "summaries" => crate::lang::t!("summaries", "摘要"),
            "sessions" => crate::lang::t!("sessions", "会话"),
            "decisions" => crate::lang::t!("decisions", "决策"),
            "unique_decisions" => crate::lang::t!("unique decisions", "去重决策"),
            other => other,
        };
        format!(" [{}/{} {}]", coverage.observed, coverage.eligible, unit)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerScore {
    pub name: String,
    pub signal: Signal,
    pub indicators: Vec<Indicator>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScoreResult {
    pub layers: [LayerScore; 3],
    pub tension: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<super::scope::ScoreScope>,
}

impl Default for ScoreResult {
    fn default() -> Self {
        Self {
            layers: default_layers(),
            tension: None,
            timestamp: DateTime::<Utc>::UNIX_EPOCH,
            scope: None,
        }
    }
}

fn default_layers() -> [LayerScore; 3] {
    [
        LayerScore {
            name: "depth".to_string(),
            signal: Signal::Unknown,
            indicators: Vec::new(),
        },
        LayerScore {
            name: "breadth".to_string(),
            signal: Signal::Unknown,
            indicators: Vec::new(),
        },
        LayerScore {
            name: "collaboration".to_string(),
            signal: Signal::Unknown,
            indicators: Vec::new(),
        },
    ]
}

/// Keep a measured failure visible, but never turn missing evidence green.
pub(super) fn worst(signals: &[Signal]) -> Signal {
    if signals.contains(&Signal::Red) {
        Signal::Red
    } else if signals.is_empty() || signals.contains(&Signal::Unknown) {
        Signal::Unknown
    } else if signals.contains(&Signal::Yellow) {
        Signal::Yellow
    } else {
        Signal::Green
    }
}
