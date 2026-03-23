use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;

use super::indicators::format_indicator_value;

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

    pub const fn ansi_dot(self) -> &'static str {
        match self {
            Signal::Green => "\x1b[32m●\x1b[0m",
            Signal::Yellow => "\x1b[33m●\x1b[0m",
            Signal::Red => "\x1b[31m●\x1b[0m",
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Indicator {
    pub name: String,
    pub actual: f64,
    pub target: String,
    pub signal: Signal,
}

impl Indicator {
    pub fn display_value(&self) -> String {
        format_indicator_value(&self.name, self.actual)
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
}

impl Default for ScoreResult {
    fn default() -> Self {
        Self {
            layers: default_layers(),
            tension: None,
            timestamp: DateTime::<Utc>::UNIX_EPOCH,
        }
    }
}

fn default_layers() -> [LayerScore; 3] {
    [
        LayerScore {
            name: "depth".to_string(),
            signal: Signal::Yellow,
            indicators: Vec::new(),
        },
        LayerScore {
            name: "breadth".to_string(),
            signal: Signal::Yellow,
            indicators: Vec::new(),
        },
        LayerScore {
            name: "collaboration".to_string(),
            signal: Signal::Yellow,
            indicators: Vec::new(),
        },
    ]
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
