use super::*;
use chrono::{DateTime, Duration, Utc};

use super::persistence::persist_score_to_path;
use crate::config::Targets;
use refine_core::session::{ClusterResult, GlobalStats, ProjectCluster};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Barrier};

fn make_cluster(
    cognitive: HashMap<String, usize>,
    collab: HashMap<String, usize>,
    decisions: usize,
    bugfixes: usize,
    projects: Vec<(&str, usize, Vec<&str>)>,
) -> ClusterResult {
    let mut project_map = HashMap::new();
    for (name, sessions, dec_titles) in &projects {
        project_map.insert(
            name.to_string(),
            ProjectCluster {
                project_name: name.to_string(),
                session_count: *sessions,
                summary_excerpts: Vec::new(),
                decision_titles: dec_titles.iter().map(|s| s.to_string()).collect(),
                bugfix_titles: Vec::new(),
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tools: Vec::new(),
                frictions: Vec::new(),
                architectures: Vec::new(),
                knowledge_gained: Vec::new(),
                patterns: Vec::new(),
            },
        );
    }
    ClusterResult {
        projects: project_map,
        global_stats: GlobalStats {
            total_sessions: 100,
            total_decisions: decisions,
            total_bugfixes: bugfixes,
            total_summaries: 200,
            cognitive_levels: cognitive,
            collaboration_modes: collab,
            tool_frequency: HashMap::new(),
            project_ranking: Vec::new(),
        },
        untagged_count: 0,
    }
}

#[allow(clippy::type_complexity)]
fn make_cluster_with_data(
    cognitive: HashMap<String, usize>,
    collab: HashMap<String, usize>,
    decisions: usize,
    bugfixes: usize,
    projects: Vec<(&str, usize, Vec<&str>, Vec<&str>, Vec<&str>)>,
) -> ClusterResult {
    let total_sessions: usize = projects.iter().map(|(_, s, _, _, _)| *s).sum();
    let mut project_map = HashMap::new();
    for (name, sessions, dec_titles, frictions, knowledge) in &projects {
        project_map.insert(
            name.to_string(),
            ProjectCluster {
                project_name: name.to_string(),
                session_count: *sessions,
                summary_excerpts: Vec::new(),
                decision_titles: dec_titles.iter().map(|s| s.to_string()).collect(),
                bugfix_titles: Vec::new(),
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tools: Vec::new(),
                frictions: frictions.iter().map(|s| s.to_string()).collect(),
                architectures: Vec::new(),
                knowledge_gained: knowledge.iter().map(|s| s.to_string()).collect(),
                patterns: Vec::new(),
            },
        );
    }
    ClusterResult {
        projects: project_map,
        global_stats: GlobalStats {
            total_sessions,
            total_decisions: decisions,
            total_bugfixes: bugfixes,
            total_summaries: 200,
            cognitive_levels: cognitive,
            collaboration_modes: collab,
            tool_frequency: HashMap::new(),
            project_ranking: Vec::new(),
        },
        untagged_count: 0,
    }
}

/// Build a ScoreResult with known indicator values for baseline testing.
#[allow(clippy::too_many_arguments)]
fn make_score_result(
    dreyfus: f64,
    decision_quality: f64,
    depth_output: f64,
    knowledge_rate_val: f64,
    exploration: f64,
    deep_invest: f64,
    fragmentation: f64,
    delegation: f64,
    mode_diversity: f64,
    bug_decision: f64,
    friction_density_val: f64,
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
                    Indicator {
                        name: "depth_output".into(),
                        actual: depth_output,
                        target: String::new(),
                        signal: Signal::Yellow,
                    },
                    Indicator {
                        name: "knowledge_rate".into(),
                        actual: knowledge_rate_val,
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
                    Indicator {
                        name: "friction_density".into(),
                        actual: friction_density_val,
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

fn rename_indicator(result: &mut ScoreResult, from: &str, to: &str) {
    for layer in &mut result.layers {
        for indicator in &mut layer.indicators {
            if indicator.name == from {
                indicator.name = to.to_string();
            }
        }
    }
}

fn remove_indicator(result: &mut ScoreResult, name: &str) {
    for layer in &mut result.layers {
        layer.indicators.retain(|indicator| indicator.name != name);
    }
}

fn convert_to_legacy_schema(result: &mut ScoreResult) {
    // Historical snapshots stored localized indicator names in some versions.
    let aliases = [
        ("decision_quality", "决策质量"),
        ("depth_output", "深度产出比"),
        ("exploration", "探索率"),
        ("deep_invest", "深挖率"),
        ("fragmentation", "碎片化"),
        ("delegation", "委派率"),
        ("mode_diversity", "模式多样性"),
        ("bug_decision", "bug/决策"),
    ];
    for (from, to) in aliases {
        rename_indicator(result, from, to);
    }

    // knowledge_rate / friction_density did not exist in older snapshots.
    remove_indicator(result, "knowledge_rate");
    remove_indicator(result, "friction_density");
}

#[test]
fn test_signal_conversions_are_canonical() {
    let cases = [
        (Signal::Green, "green", "🟢", "\x1b[32m●\x1b[0m"),
        (Signal::Yellow, "yellow", "🟡", "\x1b[33m●\x1b[0m"),
        (Signal::Red, "red", "🔴", "\x1b[31m●\x1b[0m"),
    ];

    for (signal, plain, emoji, ansi) in cases {
        assert_eq!(signal.as_str(), plain);
        assert_eq!(signal.emoji(), emoji);
        assert_eq!(signal.to_string(), ansi);
    }
}

#[test]
fn test_dreyfus_weighted_calculation() {
    let mut cog = HashMap::new();
    cog.insert("expert".into(), 5);
    cog.insert("proficient".into(), 5);
    let stats = GlobalStats {
        total_sessions: 10,
        total_decisions: 0,
        total_bugfixes: 0,
        total_summaries: 0,
        cognitive_levels: cog,
        collaboration_modes: HashMap::new(),
        tool_frequency: HashMap::new(),
        project_ranking: Vec::new(),
    };
    let dw = dreyfus_weighted(&stats);
    // (5*5 + 5*4) / 10 = 4.5
    assert!((dw - 4.5).abs() < f64::EPSILON);
}

#[test]
fn test_signal_from_thresholds() {
    let t = Targets::default();
    let cluster = make_cluster(
        {
            let mut m = HashMap::new();
            m.insert("expert".into(), 10);
            m
        },
        {
            let mut m = HashMap::new();
            m.insert("exploration".into(), 20);
            m.insert("delegation".into(), 10);
            m.insert("pair_programming".into(), 10);
            m.insert("review".into(), 10);
            m.insert("deep_inquiry".into(), 10);
            m
        },
        50,
        10,
        vec![
            ("proj-a", 25, vec!["因为性能选择 Rust", "采用 SQLite"]),
            ("proj-b", 5, vec!["修复 bug"]),
        ],
    );
    let result = compute(&cluster, &t);
    // Dreyfus = 5.0 → green
    assert_eq!(result.layers[0].indicators[0].signal, Signal::Green);
}

#[test]
fn test_layer_signal_worst_of_three() {
    let t = Targets::default();
    let cluster = make_cluster(
        {
            let mut m = HashMap::new();
            m.insert("novice".into(), 10); // dreyfus = 1.0 → red
            m
        },
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 80);
            m.insert("exploration".into(), 20);
            m
        },
        10,
        2,
        vec![("proj-a", 5, vec!["选择 X 因为 Y"])],
    );
    let result = compute(&cluster, &t);
    // layer1 dreyfus=1.0 red, so layer1 = red
    assert_eq!(result.layers[0].signal, Signal::Red);
}

#[test]
fn test_tension_analysis() {
    let green_layer = LayerScore {
        name: "test".into(),
        signal: Signal::Green,
        indicators: Vec::new(),
    };
    let red_layer = LayerScore {
        name: "test".into(),
        signal: Signal::Red,
        indicators: Vec::new(),
    };
    // L1 green + L2 red
    let tension = analyze_tension(&[green_layer.clone(), red_layer.clone(), green_layer.clone()]);
    assert!(tension.is_some());
    assert!(tension.unwrap_or_default().contains("narrowing"));

    // All green
    let tension = analyze_tension(&[
        green_layer.clone(),
        green_layer.clone(),
        green_layer.clone(),
    ]);
    assert!(tension.unwrap_or_default().contains("healthy"));

    // All red
    let tension = analyze_tension(&[red_layer.clone(), red_layer.clone(), red_layer.clone()]);
    assert!(tension.unwrap_or_default().contains("replan"));
}

#[test]
fn test_persist_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let result = ScoreResult {
        layers: [
            LayerScore {
                name: "L1".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L2".into(),
                signal: Signal::Yellow,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L3".into(),
                signal: Signal::Red,
                indicators: Vec::new(),
            },
        ],
        tension: Some("test tension".into()),
        timestamp: Utc::now(),
    };

    persist_score_to_path(&path, &result).unwrap();

    let loaded = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].layers[0].signal, Signal::Green);
    assert_eq!(loaded[0].tension.as_deref(), Some("test tension"));
}

#[test]
fn test_persist_score_rotates_to_latest_365_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    for idx in 0..370 {
        let mut result = make_score_result(
            3.5,
            60.0,
            10.0,
            0.5,
            20.0,
            25.0,
            15.0,
            30.0,
            4.0,
            0.2,
            0.8,
            Utc::now() + Duration::seconds(idx),
        );
        result.tension = Some(format!("entry-{}", idx));
        persist_score_to_path(&path, &result).unwrap();
    }

    let loaded = load_recent_scores_from_path(&path, 400).unwrap();
    assert_eq!(loaded.len(), 365);
    assert_eq!(loaded[0].tension.as_deref(), Some("entry-5"));
    assert_eq!(
        loaded.last().and_then(|s| s.tension.as_deref()),
        Some("entry-369")
    );
}

#[test]
fn test_persist_score_concurrent_writes_preserve_all_new_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    for idx in 0..365usize {
        let mut seed = make_score_result(
            3.5,
            60.0,
            10.0,
            0.5,
            20.0,
            25.0,
            15.0,
            30.0,
            4.0,
            0.2,
            0.8,
            Utc::now() - Duration::days(100) + Duration::seconds(idx as i64),
        );
        seed.tension = Some(format!("seed-{}", idx));
        persist_score_to_path(&path, &seed).unwrap();
    }

    let writer_count = 12usize;
    let barrier = Arc::new(Barrier::new(writer_count));
    let mut handles = Vec::new();

    for idx in 0..writer_count {
        let barrier = Arc::clone(&barrier);
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            let mut result = make_score_result(
                3.5,
                60.0,
                10.0,
                0.5,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.2,
                0.8,
                Utc::now() + Duration::seconds(idx as i64),
            );
            result.tension = Some(format!("writer-{}", idx));
            barrier.wait();
            persist_score_to_path(&path, &result).unwrap();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let loaded = load_recent_scores_from_path(&path, 400).unwrap();
    assert_eq!(loaded.len(), 365);
    let seen: HashSet<_> = loaded
        .into_iter()
        .filter_map(|score| score.tension)
        .collect();
    for idx in 0..writer_count {
        let key = format!("writer-{}", idx);
        assert!(seen.contains(&key), "missing {}", key);
    }
}

#[test]
fn test_load_recent_scores_reports_invalid_jsonl_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let valid = ScoreResult {
        layers: [
            LayerScore {
                name: "L1".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L2".into(),
                signal: Signal::Yellow,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L3".into(),
                signal: Signal::Red,
                indicators: Vec::new(),
            },
        ],
        tension: None,
        timestamp: Utc::now(),
    };
    let valid_line = serde_json::to_string(&valid).unwrap();
    std::fs::write(&path, format!("{}\n{{\"bad\":\n", valid_line)).unwrap();

    let err = load_recent_scores_from_path(&path, 10).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("line 2"));
    assert!(msg.contains("score history"));
}

#[test]
fn test_load_recent_scores_accepts_legacy_missing_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let legacy_line = r#"{"layers":[{"name":"L1","signal":"Green","indicators":[]},{"name":"L2","signal":"Yellow","indicators":[]},{"name":"L3","signal":"Red","indicators":[]}],"tension":"legacy tension"}"#;
    std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

    let loaded = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].tension.as_deref(), Some("legacy tension"));
    assert_eq!(loaded[0].timestamp, DateTime::<Utc>::UNIX_EPOCH);
}

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
    assert!((bl.dreyfus_avg - 3.5).abs() < f64::EPSILON);
    assert!((bl.decision_quality_avg - 60.0).abs() < f64::EPSILON);
    assert!((bl.depth_output_avg - 10.0).abs() < f64::EPSILON);
    assert!((bl.exploration_avg - 20.0).abs() < f64::EPSILON);
    assert!((bl.deep_invest_avg - 25.0).abs() < f64::EPSILON);
    assert!((bl.fragmentation_avg - 15.0).abs() < f64::EPSILON);
    assert!((bl.delegation_avg - 30.0).abs() < f64::EPSILON);
    assert!((bl.mode_diversity_avg - 4.0).abs() < f64::EPSILON);
    assert!((bl.bug_decision_avg - 0.20).abs() < f64::EPSILON);
    assert!((bl.knowledge_rate_avg - 0.5).abs() < f64::EPSILON);
    assert!((bl.friction_density_avg - 0.8).abs() < f64::EPSILON);
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
        (bl.decision_quality_avg - 80.0).abs() < f64::EPSILON,
        "mixed-schema decision_quality should stay at 80.0, got {}",
        bl.decision_quality_avg
    );
    assert!(
        (bl.knowledge_rate_avg - 0.9).abs() < f64::EPSILON,
        "missing legacy entries should not dilute knowledge_rate avg, got {}",
        bl.knowledge_rate_avg
    );
    assert!(
        (bl.friction_density_avg - 0.7).abs() < f64::EPSILON,
        "missing legacy entries should not dilute friction_density avg, got {}",
        bl.friction_density_avg
    );
}

#[test]
fn test_signal_from_personal() {
    // higher_is_better = true
    // actual = 105% of baseline → green
    assert_eq!(signal_from_personal(10.5, 10.0, true), Signal::Green);
    // actual = 100% of baseline → yellow (within ±5%)
    assert_eq!(signal_from_personal(10.0, 10.0, true), Signal::Yellow);
    // actual = 90% of baseline → red
    assert_eq!(signal_from_personal(9.0, 10.0, true), Signal::Red);

    // higher_is_better = false (lower is better)
    // actual = 90% of baseline → green (notably lower = good)
    assert_eq!(signal_from_personal(9.0, 10.0, false), Signal::Green);
    // actual = 100% of baseline → yellow
    assert_eq!(signal_from_personal(10.0, 10.0, false), Signal::Yellow);
    // actual = 110% of baseline → red (higher = bad)
    assert_eq!(signal_from_personal(11.0, 10.0, false), Signal::Red);

    // Edge: baseline == 0 → yellow
    assert_eq!(signal_from_personal(5.0, 0.0, true), Signal::Yellow);
    assert_eq!(signal_from_personal(0.0, 0.0, false), Signal::Yellow);
}

#[test]
fn test_apply_personal_baseline() {
    let now = Utc::now();

    // Baseline averages
    let baseline = PersonalBaseline {
        dreyfus_avg: 3.0,
        decision_quality_avg: 50.0,
        depth_output_avg: 10.0,
        exploration_avg: 20.0,
        deep_invest_avg: 25.0,
        fragmentation_avg: 15.0, // lower is better
        delegation_avg: 30.0,    // lower is better
        mode_diversity_avg: 4.0,
        bug_decision_avg: 0.20, // lower is better
        knowledge_rate_avg: 0.5,
        friction_density_avg: 1.0, // lower is better
    };

    // Current score: all significantly above baseline
    let mut result = make_score_result(
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

    apply_personal_baseline(&mut result, &baseline);

    // All indicators should be green
    for layer in &result.layers {
        for indicator in &layer.indicators {
            assert_eq!(
                indicator.signal,
                Signal::Green,
                "expected green for {} (actual={})",
                indicator.name,
                indicator.actual,
            );
        }
        assert_eq!(layer.signal, Signal::Green);
    }
}

#[test]
fn test_apply_personal_baseline_regression() {
    let now = Utc::now();

    let baseline = PersonalBaseline {
        dreyfus_avg: 4.0,
        decision_quality_avg: 70.0,
        depth_output_avg: 15.0,
        exploration_avg: 25.0,
        deep_invest_avg: 30.0,
        fragmentation_avg: 10.0,
        delegation_avg: 20.0,
        mode_diversity_avg: 5.0,
        bug_decision_avg: 0.15,
        knowledge_rate_avg: 0.8,
        friction_density_avg: 0.5,
    };

    // Current: all significantly worse than baseline
    let mut result = make_score_result(
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

    apply_personal_baseline(&mut result, &baseline);

    for layer in &result.layers {
        for indicator in &layer.indicators {
            assert_eq!(
                indicator.signal,
                Signal::Red,
                "expected red for {} (actual={})",
                indicator.name,
                indicator.actual,
            );
        }
        assert_eq!(layer.signal, Signal::Red);
    }
}

#[test]
fn test_knowledge_rate_green() {
    let t = Targets::default();
    // 10 sessions, 6 knowledge items -> rate = 0.6 >= 0.5 -> green
    let cluster = make_cluster_with_data(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec![],
                vec!["learned Rust", "learned SQL", "learned async"],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec![],
                vec!["learned Docker", "learned K8s", "learned Nix"],
            ),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.6).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1
        .indicators
        .iter()
        .find(|i| i.name == "knowledge_rate")
        .unwrap();
    assert_eq!(kr_ind.signal, Signal::Green);
}

#[test]
fn test_knowledge_rate_red() {
    let t = Targets::default();
    // 10 sessions, 1 knowledge item -> rate = 0.1 < 0.2 -> red
    let cluster = make_cluster_with_data(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            ("proj-a", 5, vec![], vec![], vec!["learned Rust"]),
            ("proj-b", 5, vec![], vec![], vec![]),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.1).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1
        .indicators
        .iter()
        .find(|i| i.name == "knowledge_rate")
        .unwrap();
    assert_eq!(kr_ind.signal, Signal::Red);
}

#[test]
fn test_friction_density_green() {
    let t = Targets::default();
    // 10 sessions, 5 frictions -> density = 0.5 < 1.0 -> green
    let cluster = make_cluster_with_data(
        HashMap::new(),
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 5);
            m.insert("exploration".into(), 5);
            m.insert("pair_programming".into(), 5);
            m.insert("review".into(), 5);
            m
        },
        10,
        2,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec!["slow build", "confusing API"],
                vec![],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec!["flaky test", "bad docs", "timeout"],
                vec![],
            ),
        ],
    );
    let fd = friction_density(&cluster);
    assert!((fd - 0.5).abs() < f64::EPSILON);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3
        .indicators
        .iter()
        .find(|i| i.name == "friction_density")
        .unwrap();
    assert_eq!(fd_ind.signal, Signal::Green);
}

#[test]
fn test_friction_density_red() {
    let t = Targets::default();
    // 10 sessions, 25 frictions -> density = 2.5 > 2.0 -> red
    let cluster = make_cluster_with_data(
        HashMap::new(),
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 5);
            m.insert("exploration".into(), 5);
            m.insert("pair_programming".into(), 5);
            m.insert("review".into(), 5);
            m
        },
        10,
        2,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec![
                    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
                    "f13",
                ],
                vec![],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec![
                    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
                ],
                vec![],
            ),
        ],
    );
    let fd = friction_density(&cluster);
    assert!(fd > 2.0);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3
        .indicators
        .iter()
        .find(|i| i.name == "friction_density")
        .unwrap();
    assert_eq!(fd_ind.signal, Signal::Red);
}

#[test]
fn test_layer1_has_4_indicators() {
    let t = Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let l1 = layer1(&cluster, &t);
    assert_eq!(l1.indicators.len(), 4);
    assert_eq!(l1.indicators[3].name, "knowledge_rate");
}

#[test]
fn test_layer3_has_4_indicators() {
    let t = Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let l3 = layer3(&cluster, &t);
    assert_eq!(l3.indicators.len(), 4);
    assert_eq!(l3.indicators[3].name, "friction_density");
}
