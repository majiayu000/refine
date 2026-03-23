use super::compute::analyze_tension;
use super::types::{
    worst, PersonalBaseline, ScoreResult, Signal, BASELINE_MIN_ENTRIES, BASELINE_WINDOW_DAYS,
};
use chrono::{Duration, Utc};

/// Extract a named indicator's actual value from a ScoreResult.
fn extract_indicator(result: &ScoreResult, name: &str) -> Option<f64> {
    result
        .layers
        .iter()
        .flat_map(|l| &l.indicators)
        .find(|i| i.name == name)
        .map(|i| i.actual)
}

/// Compute personal baseline from historical scores within the last 28 days.
/// Returns None if fewer than BASELINE_MIN_ENTRIES scores exist in that window.
pub fn compute_personal_baseline(history: &[ScoreResult]) -> Option<PersonalBaseline> {
    let cutoff = Utc::now() - Duration::days(BASELINE_WINDOW_DAYS);
    let recent: Vec<&ScoreResult> = history.iter().filter(|s| s.timestamp >= cutoff).collect();

    if recent.len() < BASELINE_MIN_ENTRIES {
        return None;
    }

    let n = recent.len() as f64;

    let avg = |name: &str| -> f64 {
        let sum: f64 = recent
            .iter()
            .filter_map(|s| extract_indicator(s, name))
            .sum();
        sum / n
    };

    Some(PersonalBaseline {
        dreyfus_avg: avg("dreyfus"),
        decision_quality_avg: avg("decision_quality"),
        depth_output_avg: avg("depth_output"),
        exploration_avg: avg("exploration"),
        deep_invest_avg: avg("deep_invest"),
        fragmentation_avg: avg("fragmentation"),
        delegation_avg: avg("delegation"),
        mode_diversity_avg: avg("mode_diversity"),
        bug_decision_avg: avg("bug_decision"),
        knowledge_rate_avg: avg("knowledge_rate"),
        friction_density_avg: avg("friction_density"),
    })
}

/// Determine signal by comparing actual value against personal baseline.
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
        if ratio <= 0.95 {
            Signal::Green
        } else if ratio <= 1.05 {
            Signal::Yellow
        } else {
            Signal::Red
        }
    }
}

struct IndicatorMeta {
    name: &'static str,
    baseline_value: f64,
    higher_is_better: bool,
}

/// Apply personal baseline to override signals on a ScoreResult (in-place).
pub(super) fn apply_personal_baseline(result: &mut ScoreResult, baseline: &PersonalBaseline) {
    let metas = [
        IndicatorMeta {
            name: "dreyfus",
            baseline_value: baseline.dreyfus_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "decision_quality",
            baseline_value: baseline.decision_quality_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "depth_output",
            baseline_value: baseline.depth_output_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "exploration",
            baseline_value: baseline.exploration_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "deep_invest",
            baseline_value: baseline.deep_invest_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "fragmentation",
            baseline_value: baseline.fragmentation_avg,
            higher_is_better: false,
        },
        IndicatorMeta {
            name: "delegation",
            baseline_value: baseline.delegation_avg,
            higher_is_better: false,
        },
        IndicatorMeta {
            name: "mode_diversity",
            baseline_value: baseline.mode_diversity_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "bug_decision",
            baseline_value: baseline.bug_decision_avg,
            higher_is_better: false,
        },
        IndicatorMeta {
            name: "knowledge_rate",
            baseline_value: baseline.knowledge_rate_avg,
            higher_is_better: true,
        },
        IndicatorMeta {
            name: "friction_density",
            baseline_value: baseline.friction_density_avg,
            higher_is_better: false,
        },
    ];

    for layer in &mut result.layers {
        for indicator in &mut layer.indicators {
            if let Some(meta) = metas.iter().find(|m| m.name == indicator.name) {
                indicator.signal = signal_from_personal(
                    indicator.actual,
                    meta.baseline_value,
                    meta.higher_is_better,
                );
            }
        }
        let sigs: Vec<Signal> = layer.indicators.iter().map(|i| i.signal).collect();
        layer.signal = worst(&sigs);
    }

    result.tension = analyze_tension(&result.layers);
}
