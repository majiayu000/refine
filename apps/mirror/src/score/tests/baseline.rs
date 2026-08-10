use super::*;
use chrono::{Duration, Utc};
use std::collections::HashMap;

#[test]
fn test_personal_baseline_calculation() {
    let now = Utc::now();
    // Build 10 historical scores within the last 28 days
    let history: Vec<ScoreResult> = (0..10)
        .map(|i| {
            make_score_result(
                3.5,  // dreyfus
                60.0, // decision_quality (stored as percentage)
                10.0, // depth_output (stored as percentage)
                0.5,  // knowledge_rate
                20.0, // exploration (stored as percentage)
                25.0, // deep_invest (stored as percentage)
                15.0, // fragmentation (stored as percentage)
                30.0, // delegation (stored as percentage)
                4.0,  // mode_diversity
                0.20, // bug_decision
                0.8,  // friction_density
                now - Duration::days(i),
            )
        })
        .collect();

    let baseline = compute_personal_baseline(&history);
    assert!(
        baseline.is_some(),
        "should produce baseline with 10 entries"
    );

    let bl = baseline.unwrap();
    assert!((bl.average("dreyfus").unwrap() - 3.5).abs() < f64::EPSILON);
    assert!((bl.average("decision_quality").unwrap() - 60.0).abs() < f64::EPSILON);
    assert!((bl.average("depth_output").unwrap() - 10.0).abs() < f64::EPSILON);
    assert!((bl.average("exploration").unwrap() - 20.0).abs() < f64::EPSILON);
    assert!((bl.average("deep_invest").unwrap() - 25.0).abs() < f64::EPSILON);
    assert!((bl.average("fragmentation").unwrap() - 15.0).abs() < f64::EPSILON);
    assert!((bl.average("delegation").unwrap() - 30.0).abs() < f64::EPSILON);
    assert!((bl.average("mode_diversity").unwrap() - 4.0).abs() < f64::EPSILON);
    assert!((bl.average("bug_decision").unwrap() - 0.20).abs() < f64::EPSILON);
    assert!((bl.average("knowledge_rate").unwrap() - 0.5).abs() < f64::EPSILON);
    assert!((bl.average("friction_density").unwrap() - 0.8).abs() < f64::EPSILON);
}

#[test]
fn test_computed_indicators_have_registry_backed_metadata() {
    let t = crate::config::Targets::default();
    let now = Utc::now();
    let score = compute(
        &make_cluster_with_data(
            {
                let mut m = HashMap::new();
                m.insert("expert".into(), 3);
                m.insert("proficient".into(), 2);
                m
            },
            {
                let mut m = HashMap::new();
                m.insert("exploration".into(), 12);
                m.insert("delegation".into(), 8);
                m.insert("pair_programming".into(), 4);
                m.insert("review".into(), 3);
                m.insert("deep_inquiry".into(), 6);
                m
            },
            16,
            5,
            vec![
                (
                    "proj-a",
                    6,
                    vec!["because caching helps", "采用 sqlite"],
                    vec!["slow ci", "flaky test"],
                    vec!["learned retry policy", "learned tracing"],
                ),
                (
                    "proj-b",
                    4,
                    vec!["选择 rust"],
                    vec!["timeout"],
                    vec!["learned batching"],
                ),
            ],
        ),
        &t,
    );

    let history: Vec<ScoreResult> = (0..7)
        .map(|i| {
            let mut entry = score.clone();
            entry.timestamp = now - Duration::days(i);
            entry
        })
        .collect();
    let baseline = compute_personal_baseline(&history).expect("baseline should exist");

    for layer in &score.layers {
        for indicator in &layer.indicators {
            assert_ne!(
                indicator_display(&indicator.name),
                "unknown",
                "indicator {} missing display metadata",
                indicator.name
            );
            assert!(
                !indicator.display_value().is_empty(),
                "indicator {} missing format metadata",
                indicator.name
            );
            assert!(
                baseline.average(&indicator.name).is_some(),
                "indicator {} missing baseline metadata",
                indicator.name
            );
        }
    }
}

#[test]
fn test_personal_baseline_insufficient_data() {
    let now = Utc::now();
    // Only 5 entries — below BASELINE_MIN_ENTRIES (7)
    let history: Vec<ScoreResult> = (0..5)
        .map(|i| {
            make_score_result(
                3.5,
                60.0,
                10.0,
                0.5,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.20,
                0.8,
                now - Duration::days(i),
            )
        })
        .collect();

    let baseline = compute_personal_baseline(&history);
    assert!(
        baseline.is_none(),
        "should return None with fewer than 7 entries"
    );
}

#[test]
fn test_personal_baseline_old_data_excluded() {
    let now = Utc::now();
    // 10 entries but all older than 28 days
    let history: Vec<ScoreResult> = (0..10)
        .map(|i| {
            make_score_result(
                3.5,
                60.0,
                10.0,
                0.5,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.20,
                0.8,
                now - Duration::days(30 + i),
            )
        })
        .collect();

    let baseline = compute_personal_baseline(&history);
    assert!(
        baseline.is_none(),
        "should return None when all data is outside 28-day window"
    );
}

#[test]
fn test_personal_baseline_mixed_legacy_schema_repro() {
    let now = Utc::now();
    let mut history = Vec::new();

    for i in 0..7 {
        let mut legacy = make_score_result(
            4.0,
            80.0,
            20.0,
            0.0,
            30.0,
            25.0,
            10.0,
            15.0,
            5.0,
            0.10,
            0.0,
            now - Duration::days(i),
        );
        convert_to_legacy_schema(&mut legacy);
        history.push(legacy);
    }

    for i in 7..10 {
        history.push(make_score_result(
            4.0,
            80.0,
            20.0,
            0.9,
            30.0,
            25.0,
            10.0,
            15.0,
            5.0,
            0.10,
            0.7,
            now - Duration::days(i),
        ));
    }

    let bl = compute_personal_baseline(&history)
        .expect("baseline should be produced with 10 recent entries");

    // Expected baseline should be stable despite mixed history schema.
    assert!(
        (bl.average("decision_quality").unwrap() - 80.0).abs() < f64::EPSILON,
        "mixed-schema decision_quality should stay at 80.0, got {}",
        bl.average("decision_quality").unwrap()
    );
    assert!(
        (bl.average("knowledge_rate").unwrap() - 0.9).abs() < f64::EPSILON,
        "missing legacy entries should not dilute knowledge_rate avg, got {}",
        bl.average("knowledge_rate").unwrap()
    );
    assert!(
        (bl.average("friction_density").unwrap() - 0.7).abs() < f64::EPSILON,
        "missing legacy entries should not dilute friction_density avg, got {}",
        bl.average("friction_density").unwrap()
    );
}

#[test]
fn test_trend_from_personal() {
    use crate::score::indicators::Direction;

    assert_eq!(
        trend_from_personal(10.5, 10.0, Direction::HigherBetter),
        Some(Trend::Up)
    );
    assert_eq!(
        trend_from_personal(10.0, 10.0, Direction::HigherBetter),
        Some(Trend::Flat)
    );
    assert_eq!(
        trend_from_personal(9.0, 10.0, Direction::HigherBetter),
        Some(Trend::Down)
    );
    assert_eq!(
        trend_from_personal(9.0, 10.0, Direction::LowerBetter),
        Some(Trend::Up)
    );
    assert_eq!(
        trend_from_personal(11.0, 10.0, Direction::LowerBetter),
        Some(Trend::Down)
    );
    assert_eq!(trend_from_personal(20.0, 10.0, Direction::Band), None);
    assert_eq!(trend_from_personal(5.0, 0.0, Direction::HigherBetter), None);
}

#[test]
fn test_personal_trends_do_not_override_absolute_signals() {
    let now = Utc::now();

    // Baseline averages
    let baseline = PersonalBaseline::from_averages(&[
        ("dreyfus", 3.0),
        ("decision_quality", 50.0),
        ("depth_output", 10.0),
        ("exploration", 20.0),
        ("deep_invest", 25.0),
        ("fragmentation", 15.0), // lower is better
        ("delegation", 30.0),    // lower is better
        ("mode_diversity", 4.0),
        ("bug_decision", 0.20), // lower is better
        ("knowledge_rate", 0.5),
        ("friction_density", 1.0), // lower is better
    ]);

    let result = make_score_result(
        3.5,  // dreyfus: 3.5/3.0 = 1.167 → green (higher is better)
        60.0, // dq: 60/50 = 1.20 → green
        12.0, // do: 12/10 = 1.20 → green
        0.7,  // kr: 0.7/0.5 = 1.40 → green (higher is better)
        25.0, // exp: 25/20 = 1.25 → green
        30.0, // di: 30/25 = 1.20 → green
        10.0, // frag: 10/15 = 0.667 → green (lower is better, ratio < 0.95)
        20.0, // del: 20/30 = 0.667 → green (lower is better)
        5.0,  // md: 5/4 = 1.25 → green
        0.10, // bug: 0.10/0.20 = 0.50 → green (lower is better)
        0.5,  // fd: 0.5/1.0 = 0.50 → green (lower is better)
        now,
    );

    let signals_before: Vec<Signal> = result
        .layers
        .iter()
        .flat_map(|layer| layer.indicators.iter().map(|indicator| indicator.signal))
        .collect();
    let trends = compute_personal_trends(&result, &baseline);
    let signals_after: Vec<Signal> = result
        .layers
        .iter()
        .flat_map(|layer| layer.indicators.iter().map(|indicator| indicator.signal))
        .collect();

    assert_eq!(signals_after, signals_before);
    assert_eq!(trends.indicator("dreyfus"), Some(Trend::Up));
    assert_eq!(trends.indicator("fragmentation"), Some(Trend::Up));
    assert_eq!(trends.indicator("deep_invest"), None);
    assert_eq!(trends.overall(), Some(Trend::Up));
}

#[test]
fn test_personal_trends_detect_regression_without_recoloring() {
    let now = Utc::now();

    let baseline = PersonalBaseline::from_averages(&[
        ("dreyfus", 4.0),
        ("decision_quality", 70.0),
        ("depth_output", 15.0),
        ("exploration", 25.0),
        ("deep_invest", 30.0),
        ("fragmentation", 10.0),
        ("delegation", 20.0),
        ("mode_diversity", 5.0),
        ("bug_decision", 0.15),
        ("knowledge_rate", 0.8),
        ("friction_density", 0.5),
    ]);

    // Current: all significantly worse than baseline
    let result = make_score_result(
        3.0,  // dreyfus: 3.0/4.0 = 0.75 → red
        50.0, // dq: 50/70 = 0.71 → red
        10.0, // do: 10/15 = 0.67 → red
        0.3,  // kr: 0.3/0.8 = 0.375 → red (higher is better)
        18.0, // exp: 18/25 = 0.72 → red
        20.0, // di: 20/30 = 0.67 → red
        15.0, // frag: 15/10 = 1.50 → red (lower is better, ratio > 1.05)
        30.0, // del: 30/20 = 1.50 → red
        3.0,  // md: 3/5 = 0.60 → red
        0.30, // bug: 0.30/0.15 = 2.0 → red
        1.5,  // fd: 1.5/0.5 = 3.0 → red (lower is better)
        now,
    );

    let layer_signals = result.layers.clone().map(|layer| layer.signal);
    let trends = compute_personal_trends(&result, &baseline);

    assert_eq!(
        result.layers.clone().map(|layer| layer.signal),
        layer_signals
    );
    assert_eq!(trends.indicator("dreyfus"), Some(Trend::Down));
    assert_eq!(trends.indicator("fragmentation"), Some(Trend::Down));
    assert_eq!(trends.indicator("deep_invest"), None);
    assert_eq!(trends.overall(), Some(Trend::Down));
}
