use chrono::{Duration, Utc};
use std::collections::HashMap;

use super::indicators::{canonical_indicator_key, indicator_direction, indicator_specs, Direction};
use super::types::{ScoreResult, Trend};

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

/// Compare the current value with the rolling personal baseline. Band-targeted
/// metrics do not have a meaningful monotonic direction and return `None`.
pub(super) fn trend_from_personal(
    actual: f64,
    baseline: f64,
    direction: Direction,
) -> Option<Trend> {
    if baseline == 0.0 {
        return None;
    }
    let ratio = actual / baseline;
    match direction {
        Direction::HigherBetter => Some(if ratio >= 1.05 {
            Trend::Up
        } else if ratio <= 0.95 {
            Trend::Down
        } else {
            Trend::Flat
        }),
        Direction::LowerBetter => Some(if ratio <= 0.95 {
            Trend::Up
        } else if ratio >= 1.05 {
            Trend::Down
        } else {
            Trend::Flat
        }),
        Direction::Band => None,
    }
}

#[derive(Debug, Clone, Default)]
pub struct PersonalTrends {
    per_indicator: HashMap<String, Trend>,
    per_layer: [Option<Trend>; 3],
}

impl PersonalTrends {
    pub fn indicator(&self, name: &str) -> Option<Trend> {
        self.per_indicator
            .get(canonical_indicator_key(name))
            .copied()
    }

    pub fn overall(&self) -> Option<Trend> {
        aggregate(self.per_layer.iter().copied().flatten())
    }
}

fn aggregate(trends: impl Iterator<Item = Trend>) -> Option<Trend> {
    let mut has_up = false;
    let mut has_down = false;
    let mut any = false;
    for trend in trends {
        any = true;
        match trend {
            Trend::Up => has_up = true,
            Trend::Down => has_down = true,
            Trend::Flat => {}
        }
    }
    if !any {
        return None;
    }
    Some(match (has_up, has_down) {
        (true, false) => Trend::Up,
        (false, true) => Trend::Down,
        _ => Trend::Flat,
    })
}

/// Derive a read-only personal trend view. Absolute target signals remain
/// untouched and retain the same meaning in daily, weekly, and persisted data.
pub(super) fn compute_personal_trends(
    result: &ScoreResult,
    baseline: &PersonalBaseline,
) -> PersonalTrends {
    let mut per_indicator = HashMap::new();
    let mut per_layer = [None, None, None];

    for (index, layer) in result.layers.iter().enumerate() {
        for indicator in &layer.indicators {
            let key = canonical_indicator_key(&indicator.name);
            let (Some(direction), Some(baseline_value)) =
                (indicator_direction(key), baseline.average(key))
            else {
                continue;
            };
            if let Some(trend) = trend_from_personal(indicator.actual, baseline_value, direction) {
                per_indicator.insert(key.to_string(), trend);
            }
        }

        per_layer[index] = aggregate(layer.indicators.iter().filter_map(|indicator| {
            per_indicator
                .get(canonical_indicator_key(&indicator.name))
                .copied()
        }));
    }

    PersonalTrends {
        per_indicator,
        per_layer,
    }
}
