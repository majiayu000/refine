use chrono::{Duration, Utc};
use std::collections::HashMap;

use super::compute::analyze_tension;
use super::indicators::{canonical_indicator_key, indicator_higher_is_better, indicator_specs};
use super::types::{worst, ScoreResult, Signal};

/// Minimum number of historical scores to activate personal baseline
const BASELINE_MIN_ENTRIES: usize = 7;

/// Sliding window size in days
const BASELINE_WINDOW_DAYS: i64 = 28;

/// 4-week rolling averages for each indicator
#[derive(Debug, Clone, Default)]
pub struct PersonalBaseline {
    averages: HashMap<String, f64>,
}

impl PersonalBaseline {
    pub fn average(&self, name: &str) -> Option<f64> {
        self.averages.get(name).copied()
    }

    #[cfg(test)]
    pub fn from_averages(entries: &[(&str, f64)]) -> Self {
        Self {
            averages: entries
                .iter()
                .map(|(name, value)| ((*name).to_string(), *value))
                .collect(),
        }
    }
}

/// Extract a named indicator's actual value from a ScoreResult.
/// Returns None if the indicator is not found.
fn extract_indicator(result: &ScoreResult, name: &str) -> Option<f64> {
    result
        .layers
        .iter()
        .flat_map(|l| &l.indicators)
        .find(|i| canonical_indicator_key(&i.name) == name)
        .map(|i| i.actual)
}

/// Compute personal baseline from historical scores within the last 28 days.
/// Returns None if fewer than BASELINE_MIN_ENTRIES scores exist in that window.
fn avg_from_scores(scores: &[&ScoreResult], indicator_name: &str) -> f64 {
    let (sum, count) = scores
        .iter()
        .filter_map(|score| extract_indicator(score, indicator_name))
        .fold((0.0, 0usize), |(sum, count), value| {
            (sum + value, count + 1)
        });

    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

pub fn compute_personal_baseline(history: &[ScoreResult]) -> Option<PersonalBaseline> {
    let cutoff = Utc::now() - Duration::days(BASELINE_WINDOW_DAYS);
    let recent: Vec<&ScoreResult> = history.iter().filter(|s| s.timestamp >= cutoff).collect();

    if recent.len() < BASELINE_MIN_ENTRIES {
        return None;
    }

    let averages = indicator_specs()
        .iter()
        .map(|spec| (spec.key.to_string(), avg_from_scores(&recent, spec.key)))
        .collect();

    Some(PersonalBaseline { averages })
}

/// Determine signal by comparing actual value against personal baseline.
/// `higher_is_better`: true for metrics where higher = better (dreyfus, decision_quality, etc.)
///                     false for metrics where lower = better (delegation, fragmentation, bug_decision)
pub fn signal_from_personal(actual: f64, baseline: f64, higher_is_better: bool) -> Signal {
    if baseline == 0.0 {
        return Signal::Yellow;
    }
    let ratio = actual / baseline;
    if higher_is_better {
        if ratio >= 1.05 {
            Signal::Green
        } else if ratio >= 0.95 {
            Signal::Yellow
        } else {
            Signal::Red
        }
    } else {
        // Lower is better: ratio < 0.95 means actual is notably below baseline = good
        if ratio <= 0.95 {
            Signal::Green
        } else if ratio <= 1.05 {
            Signal::Yellow
        } else {
            Signal::Red
        }
    }
}

/// Apply personal baseline to override signals on a ScoreResult (in-place).
pub(super) fn apply_personal_baseline(result: &mut ScoreResult, baseline: &PersonalBaseline) {
    for layer in &mut result.layers {
        for indicator in &mut layer.indicators {
            let key = canonical_indicator_key(&indicator.name);
            let Some(higher_is_better) = indicator_higher_is_better(key) else {
                continue;
            };
            let Some(baseline_value) = baseline.average(key) else {
                continue;
            };

            indicator.signal =
                signal_from_personal(indicator.actual, baseline_value, higher_is_better);
        }

        // Recalculate layer signal as worst of its indicators
        let sigs: Vec<Signal> = layer.indicators.iter().map(|i| i.signal).collect();
        layer.signal = worst(&sigs);
    }

    // Recalculate tension after signal changes
    result.tension = analyze_tension(&result.layers);
}
