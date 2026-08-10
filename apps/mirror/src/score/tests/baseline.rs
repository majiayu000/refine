use super::*;
use crate::config::Targets;
use crate::score::indicators::Direction;
use crate::score::types::Trend;
use chrono::{Duration, Utc};

fn make_current_score_result(
    dreyfus: f64,
    decision_quality: f64,
    exploration: f64,
    deep_invest: f64,
    fragmentation: f64,
    delegation: f64,
    mode_diversity: f64,
    bug_decision: f64,
    timestamp: DateTime<Utc>,
) -> ScoreResult {
    ScoreResult {
        layers: [
            LayerScore {
                name: "depth".into(),
                signal: Signal::Yellow,
                indicators: vec![
                    Indicator {
                        name: "dreyfus".into(),
                        actual: dreyfus,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "decision_quality".into(),
                        actual: decision_quality,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                ],
            },
            LayerScore {
                name: "breadth".into(),
                signal: Signal::Yellow,
                indicators: vec![
                    Indicator {
                        name: "exploration".into(),
                        actual: exploration,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "deep_invest".into(),
                        actual: deep_invest,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "fragmentation".into(),
                        actual: fragmentation,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                ],
            },
            LayerScore {
                name: "collaboration".into(),
                signal: Signal::Yellow,
                indicators: vec![
                    Indicator {
                        name: "delegation".into(),
                        actual: delegation,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "mode_diversity".into(),
                        actual: mode_diversity,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "bug_decision".into(),
                        actual: bug_decision,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                ],
            },
        ],
        tension: None,
        timestamp,
    }
}

#[test]
fn test_personal_baseline_calculation() {
    let now = Utc::now();
    // Build 10 historical scores within the last 28 days
    let history: Vec<ScoreResult> = (0..10)
        .map(|i| {
            make_current_score_result(
                3.5,  // dreyfus
                60.0, // knowledge_rate
                20.0, // exploration (stored as percentage)
                25.0, // deep_invest (stored as percentage)
                15.0, // fragmentation (stored as percentage)
                30.0, // delegation (stored as percentage)
                4.0,  // mode_diversity
                0.20, // friction_density
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
    assert!((bl.average("exploration").unwrap() - 20.0).abs() < f64::EPSILON);
    assert!((bl.average("deep_invest").unwrap() - 25.0).abs() < f64::EPSILON);
    assert!((bl.average("fragmentation").unwrap() - 15.0).abs() < f64::EPSILON);
    assert!((bl.average("delegation").unwrap() - 30.0).abs() < f64::EPSILON);
    assert!((bl.average("mode_diversity").unwrap() - 4.0).abs() < f64::EPSILON);
    assert!((bl.average("bug_decision").unwrap() - 0.20).abs() < f64::EPSILON);
}

#[test]
fn test_computed_indicators_have_registry_backed_metadata() {
    let t = Targets::default();
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
            make_current_score_result(
                3.5,
                60.0,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.20,
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
            make_current_score_result(
                3.5,
                60.0,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.20,
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
        let mut legacy = make_current_score_result(
            4.0,
            80.0,
            30.0,
            25.0,
            10.0,
            15.0,
            5.0,
            0.10,
            now - Duration::days(i),
        );
        convert_to_legacy_schema(&mut legacy);
        history.push(legacy);
    }

    for i in 7..10 {
        history.push(make_current_score_result(
            4.0,
            80.0,
            30.0,
            25.0,
            10.0,
            15.0,
            5.0,
            0.10,
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
        (bl.average("bug_decision").unwrap() - 0.10).abs() < f64::EPSILON,
        "missing legacy entries should not dilute bug_decision avg, got {}",
        bl.average("bug_decision").unwrap()
    );
}

#[test]
fn test_trend_from_personal() {
    // higher is better：>=105% 均值算进步
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

    // lower is better：低于均值 5% 算进步
    assert_eq!(
        trend_from_personal(9.0, 10.0, Direction::LowerBetter),
        Some(Trend::Up)
    );
    assert_eq!(
        trend_from_personal(10.0, 10.0, Direction::LowerBetter),
        Some(Trend::Flat)
    );
    assert_eq!(
        trend_from_personal(11.0, 10.0, Direction::LowerBetter),
        Some(Trend::Down)
    );
}

#[test]
fn test_trend_none_when_baseline_zero() {
    // 基线为 0 = 窗口内无历史数据，必须显式表达"无法判断"，
    // 不能像旧的 signal_from_personal 那样伪装成 Yellow（静默降级）。
    assert_eq!(trend_from_personal(5.0, 0.0, Direction::HigherBetter), None);
    assert_eq!(trend_from_personal(0.0, 0.0, Direction::LowerBetter), None);
}

#[test]
fn test_deep_invest_band_has_no_trend() {
    // deep_invest 是区间指标（15-30% 为佳），越高不等于越好，
    // 与滑动均值比大小没有意义，必须返回 None 而不是把越界高值判成进步。
    assert_eq!(trend_from_personal(90.0, 25.0, Direction::Band), None);

    let now = Utc::now();
    let result = make_current_score_result(3.5, 60.0, 25.0, 90.0, 10.0, 20.0, 5.0, 0.1, now);
    let baseline = PersonalBaseline::from_averages(&[("deep_invest", 25.0)]);
    let trends = compute_personal_trends(&result, &baseline);
    assert_eq!(trends.indicator("deep_invest"), None);
}

/// 交叉场景 A：客观达标，但相对自己近 4 周在退步。
#[test]
fn test_absolute_pass_but_trend_down() {
    // 全部 proficient(权重 4.0) → dreyfus = 4.0，超过绝对目标 3.5
    let cluster = make_cluster_with_data(
        HashMap::from([("proficient".to_string(), 10usize)]),
        HashMap::new(),
        0,
        0,
        vec![
            ("proj-a", 1, vec![], vec![], vec![]),
            ("proj-b", 1, vec![], vec![], vec![]),
        ],
    );
    let result = compute(&cluster, &Targets::default());
    let kr = result.layers[0]
        .indicators
        .iter()
        .find(|i| i.name == "dreyfus")
        .expect("dreyfus indicator missing");

    assert!((kr.actual - 4.0).abs() < f64::EPSILON);
    assert_eq!(kr.signal, Signal::Green, "绝对达标必须是 Green");
    assert_eq!(kr.target, ">3.5", "target 必须与绝对判据同源");

    // 个人 4 周均值 5.0 → 4.0/5.0 = 0.8 → 相对退步
    let baseline = PersonalBaseline::from_averages(&[("dreyfus", 5.0)]);
    let trends = compute_personal_trends(&result, &baseline);
    assert_eq!(trends.indicator("dreyfus"), Some(Trend::Down));

    // 关键：趋势不得吞掉绝对信号
    let after = result.layers[0]
        .indicators
        .iter()
        .find(|i| i.name == "dreyfus")
        .expect("dreyfus indicator missing");
    assert_eq!(after.signal, Signal::Green);
}

/// 交叉场景 B：客观不达标，但相对自己近 4 周在进步。
#[test]
fn test_absolute_fail_but_trend_up() {
    // 投入加权口径：单 session 项目共 5 个会话，总会话 10 → fragmentation = 50%，
    // 超过 yellow 线 40% → Red。（桶计数口径下同一组数据会得到 5/6=83%，
    // 这个 fixture 因此也钉住了"按会话加权而非按桶加权"这一语义。）
    let cluster = make_cluster_with_data(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            ("solo-a", 1, vec![], vec![], vec![]),
            ("solo-b", 1, vec![], vec![], vec![]),
            ("solo-c", 1, vec![], vec![], vec![]),
            ("solo-d", 1, vec![], vec![], vec![]),
            ("solo-e", 1, vec![], vec![], vec![]),
            ("deep-a", 5, vec![], vec![], vec![]),
        ],
    );
    let result = compute(&cluster, &Targets::default());
    let frag = result.layers[1]
        .indicators
        .iter()
        .find(|i| i.name == "fragmentation")
        .expect("fragmentation indicator missing");

    assert!((frag.actual - 50.0).abs() < f64::EPSILON);
    assert_eq!(frag.signal, Signal::Red, "超过 yellow 线必须是 Red");
    assert_eq!(frag.target, "<20%");

    // 个人 4 周均值 60% → 50/60 = 0.833，越低越好 → 相对进步
    let baseline = PersonalBaseline::from_averages(&[("fragmentation", 60.0)]);
    let trends = compute_personal_trends(&result, &baseline);
    assert_eq!(trends.indicator("fragmentation"), Some(Trend::Up));
}

#[test]
fn test_personal_trend_never_mutates_signal() {
    let now = Utc::now();
    let baseline = PersonalBaseline::from_averages(&[
        ("dreyfus", 3.0),
        ("decision_quality", 50.0),
        ("depth_output", 10.0),
        ("exploration", 20.0),
        ("deep_invest", 25.0),
        ("fragmentation", 15.0),
        ("delegation", 30.0),
        ("mode_diversity", 4.0),
        ("bug_decision", 0.20),
        ("knowledge_rate", 0.5),
        ("friction_density", 1.0),
    ]);
    let result = make_current_score_result(3.5, 60.0, 25.0, 30.0, 10.0, 20.0, 5.0, 0.10, now);

    let signals_before: Vec<Signal> = result
        .layers
        .iter()
        .flat_map(|l| l.indicators.iter().map(|i| i.signal))
        .collect();
    let layer_signals_before: Vec<Signal> = result.layers.iter().map(|l| l.signal).collect();
    let tension_before = result.tension.clone();

    let trends = compute_personal_trends(&result, &baseline);

    let signals_after: Vec<Signal> = result
        .layers
        .iter()
        .flat_map(|l| l.indicators.iter().map(|i| i.signal))
        .collect();
    let layer_signals_after: Vec<Signal> = result.layers.iter().map(|l| l.signal).collect();

    assert_eq!(signals_before, signals_after, "指标信号不得被趋势覆写");
    assert_eq!(
        layer_signals_before, layer_signals_after,
        "层信号不得被覆写"
    );
    assert_eq!(tension_before, result.tension, "张力必须仍基于绝对信号");

    // 趋势本身被正确填充（全部优于基线）
    assert_eq!(trends.indicator("dreyfus"), Some(Trend::Up));
    assert_eq!(trends.indicator("fragmentation"), Some(Trend::Up));
    assert_eq!(trends.overall(), Some(Trend::Up));
}

#[test]
fn test_personal_trends_all_down() {
    let now = Utc::now();
    let baseline = PersonalBaseline::from_averages(&[
        ("dreyfus", 4.0),
        ("decision_quality", 70.0),
        ("exploration", 25.0),
        ("deep_invest", 30.0),
        ("fragmentation", 10.0),
        ("delegation", 20.0),
        ("mode_diversity", 5.0),
        ("bug_decision", 0.15),
    ]);
    // 全部明显差于基线
    let result = make_current_score_result(3.0, 50.0, 18.0, 20.0, 15.0, 30.0, 3.0, 0.30, now);

    let trends = compute_personal_trends(&result, &baseline);
    for key in [
        "dreyfus",
        "decision_quality",
        "exploration",
        "fragmentation",
        "delegation",
        "mode_diversity",
        "bug_decision",
    ] {
        assert_eq!(trends.indicator(key), Some(Trend::Down), "{} 应为退步", key);
    }
    // 区间指标不参与趋势
    assert_eq!(trends.indicator("deep_invest"), None);
    assert_eq!(trends.overall(), Some(Trend::Down));
}
