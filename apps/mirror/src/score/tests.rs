use super::baseline::{apply_personal_baseline, compute_personal_baseline, signal_from_personal};
use super::compute::{
    analyze_tension, dreyfus_weighted, friction_density, knowledge_rate, layer1, layer3,
};
use super::types::PersonalBaseline;
use super::*;
use chrono::{DateTime, Duration, Utc};
use refine_core::session::{ClusterResult, GlobalStats, ProjectCluster};
use std::collections::HashMap;
use std::io::{BufRead, Write};

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
                    Indicator { name: "dreyfus".into(), actual: dreyfus, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "decision_quality".into(), actual: decision_quality, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "depth_output".into(), actual: depth_output, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "knowledge_rate".into(), actual: knowledge_rate_val, target: String::new(), signal: Signal::Yellow },
                ],
            },
            LayerScore {
                name: "breadth".into(),
                signal: Signal::Yellow,
                indicators: vec![
                    Indicator { name: "exploration".into(), actual: exploration, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "deep_invest".into(), actual: deep_invest, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "fragmentation".into(), actual: fragmentation, target: String::new(), signal: Signal::Yellow },
                ],
            },
            LayerScore {
                name: "collaboration".into(),
                signal: Signal::Yellow,
                indicators: vec![
                    Indicator { name: "delegation".into(), actual: delegation, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "mode_diversity".into(), actual: mode_diversity, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "bug_decision".into(), actual: bug_decision, target: String::new(), signal: Signal::Yellow },
                    Indicator { name: "friction_density".into(), actual: friction_density_val, target: String::new(), signal: Signal::Yellow },
                ],
            },
        ],
        tension: None,
        timestamp,
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
    assert!((dw - 4.5).abs() < f64::EPSILON);
}

#[test]
fn test_signal_from_thresholds() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster(
        { let mut m = HashMap::new(); m.insert("expert".into(), 10); m },
        {
            let mut m = HashMap::new();
            m.insert("exploration".into(), 20);
            m.insert("delegation".into(), 10);
            m.insert("pair_programming".into(), 10);
            m.insert("review".into(), 10);
            m.insert("deep_inquiry".into(), 10);
            m
        },
        50, 10,
        vec![
            ("proj-a", 25, vec!["因为性能选择 Rust", "采用 SQLite"]),
            ("proj-b", 5, vec!["修复 bug"]),
        ],
    );
    let result = compute(&cluster, &t);
    assert_eq!(result.layers[0].indicators[0].signal, Signal::Green);
}

#[test]
fn test_layer_signal_worst_of_three() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster(
        { let mut m = HashMap::new(); m.insert("novice".into(), 10); m },
        { let mut m = HashMap::new(); m.insert("delegation".into(), 80); m.insert("exploration".into(), 20); m },
        10, 2,
        vec![("proj-a", 5, vec!["选择 X 因为 Y"])],
    );
    let result = compute(&cluster, &t);
    assert_eq!(result.layers[0].signal, Signal::Red);
}

#[test]
fn test_tension_analysis() {
    let green_layer = LayerScore { name: "test".into(), signal: Signal::Green, indicators: Vec::new() };
    let red_layer = LayerScore { name: "test".into(), signal: Signal::Red, indicators: Vec::new() };

    let tension = analyze_tension(&[green_layer.clone(), red_layer.clone(), green_layer.clone()]);
    assert!(tension.is_some());
    assert!(tension.unwrap_or_default().contains("narrowing"));

    let tension = analyze_tension(&[green_layer.clone(), green_layer.clone(), green_layer.clone()]);
    assert!(tension.unwrap_or_default().contains("healthy"));

    let tension = analyze_tension(&[red_layer.clone(), red_layer.clone(), red_layer.clone()]);
    assert!(tension.unwrap_or_default().contains("replan"));
}

#[test]
fn test_persist_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let result = ScoreResult {
        layers: [
            LayerScore { name: "L1".into(), signal: Signal::Green, indicators: Vec::new() },
            LayerScore { name: "L2".into(), signal: Signal::Yellow, indicators: Vec::new() },
            LayerScore { name: "L3".into(), signal: Signal::Red, indicators: Vec::new() },
        ],
        tension: Some("test tension".into()),
        timestamp: Utc::now(),
    };

    let line = serde_json::to_string(&result).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .create(true).append(true).open(&path).unwrap();
    writeln!(file, "{}", line).unwrap();
    drop(file);

    let reader = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
    let loaded: Vec<ScoreResult> = reader
        .lines()
        .filter_map(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].layers[0].signal, Signal::Green);
    assert_eq!(loaded[0].tension.as_deref(), Some("test tension"));
}

#[test]
fn test_personal_baseline_calculation() {
    let now = Utc::now();
    let history: Vec<ScoreResult> = (0..10)
        .map(|i| make_score_result(3.5, 60.0, 10.0, 0.5, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 0.8, now - Duration::days(i)))
        .collect();

    let baseline = compute_personal_baseline(&history);
    assert!(baseline.is_some(), "should produce baseline with 10 entries");

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
    let history: Vec<ScoreResult> = (0..5)
        .map(|i| make_score_result(3.5, 60.0, 10.0, 0.5, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 0.8, now - Duration::days(i)))
        .collect();
    let baseline = compute_personal_baseline(&history);
    assert!(baseline.is_none(), "should return None with fewer than 7 entries");
}

#[test]
fn test_personal_baseline_old_data_excluded() {
    let now = Utc::now();
    let history: Vec<ScoreResult> = (0..10)
        .map(|i| make_score_result(3.5, 60.0, 10.0, 0.5, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 0.8, now - Duration::days(30 + i)))
        .collect();
    let baseline = compute_personal_baseline(&history);
    assert!(baseline.is_none(), "should return None when all data is outside 28-day window");
}

#[test]
fn test_signal_from_personal() {
    assert_eq!(signal_from_personal(10.5, 10.0, true), Signal::Green);
    assert_eq!(signal_from_personal(10.0, 10.0, true), Signal::Yellow);
    assert_eq!(signal_from_personal(9.0, 10.0, true), Signal::Red);
    assert_eq!(signal_from_personal(9.0, 10.0, false), Signal::Green);
    assert_eq!(signal_from_personal(10.0, 10.0, false), Signal::Yellow);
    assert_eq!(signal_from_personal(11.0, 10.0, false), Signal::Red);
    assert_eq!(signal_from_personal(5.0, 0.0, true), Signal::Yellow);
    assert_eq!(signal_from_personal(0.0, 0.0, false), Signal::Yellow);
}

#[test]
fn test_apply_personal_baseline() {
    let now = Utc::now();
    let baseline = PersonalBaseline {
        dreyfus_avg: 3.0, decision_quality_avg: 50.0, depth_output_avg: 10.0,
        exploration_avg: 20.0, deep_invest_avg: 25.0, fragmentation_avg: 15.0,
        delegation_avg: 30.0, mode_diversity_avg: 4.0, bug_decision_avg: 0.20,
        knowledge_rate_avg: 0.5, friction_density_avg: 1.0,
    };
    let mut result = make_score_result(3.5, 60.0, 12.0, 0.7, 25.0, 30.0, 10.0, 20.0, 5.0, 0.10, 0.5, now);
    apply_personal_baseline(&mut result, &baseline);
    for layer in &result.layers {
        for indicator in &layer.indicators {
            assert_eq!(indicator.signal, Signal::Green, "expected green for {} (actual={})", indicator.name, indicator.actual);
        }
        assert_eq!(layer.signal, Signal::Green);
    }
}

#[test]
fn test_apply_personal_baseline_regression() {
    let now = Utc::now();
    let baseline = PersonalBaseline {
        dreyfus_avg: 4.0, decision_quality_avg: 70.0, depth_output_avg: 15.0,
        exploration_avg: 25.0, deep_invest_avg: 30.0, fragmentation_avg: 10.0,
        delegation_avg: 20.0, mode_diversity_avg: 5.0, bug_decision_avg: 0.15,
        knowledge_rate_avg: 0.8, friction_density_avg: 0.5,
    };
    let mut result = make_score_result(3.0, 50.0, 10.0, 0.3, 18.0, 20.0, 15.0, 30.0, 3.0, 0.30, 1.5, now);
    apply_personal_baseline(&mut result, &baseline);
    for layer in &result.layers {
        for indicator in &layer.indicators {
            assert_eq!(indicator.signal, Signal::Red, "expected red for {} (actual={})", indicator.name, indicator.actual);
        }
        assert_eq!(layer.signal, Signal::Red);
    }
}

#[test]
fn test_knowledge_rate_green() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster_with_data(
        HashMap::new(), HashMap::new(), 0, 0,
        vec![
            ("proj-a", 5, vec![], vec![], vec!["learned Rust", "learned SQL", "learned async"]),
            ("proj-b", 5, vec![], vec![], vec!["learned Docker", "learned K8s", "learned Nix"]),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.6).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1.indicators.iter().find(|i| i.name == "knowledge_rate").unwrap();
    assert_eq!(kr_ind.signal, Signal::Green);
}

#[test]
fn test_knowledge_rate_red() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster_with_data(
        HashMap::new(), HashMap::new(), 0, 0,
        vec![
            ("proj-a", 5, vec![], vec![], vec!["learned Rust"]),
            ("proj-b", 5, vec![], vec![], vec![]),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.1).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1.indicators.iter().find(|i| i.name == "knowledge_rate").unwrap();
    assert_eq!(kr_ind.signal, Signal::Red);
}

#[test]
fn test_friction_density_green() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster_with_data(
        HashMap::new(),
        { let mut m = HashMap::new(); m.insert("delegation".into(), 5); m.insert("exploration".into(), 5); m.insert("pair_programming".into(), 5); m.insert("review".into(), 5); m },
        10, 2,
        vec![
            ("proj-a", 5, vec![], vec!["slow build", "confusing API"], vec![]),
            ("proj-b", 5, vec![], vec!["flaky test", "bad docs", "timeout"], vec![]),
        ],
    );
    let fd = friction_density(&cluster);
    assert!((fd - 0.5).abs() < f64::EPSILON);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3.indicators.iter().find(|i| i.name == "friction_density").unwrap();
    assert_eq!(fd_ind.signal, Signal::Green);
}

#[test]
fn test_friction_density_red() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster_with_data(
        HashMap::new(),
        { let mut m = HashMap::new(); m.insert("delegation".into(), 5); m.insert("exploration".into(), 5); m.insert("pair_programming".into(), 5); m.insert("review".into(), 5); m },
        10, 2,
        vec![
            ("proj-a", 5, vec![], vec!["f1","f2","f3","f4","f5","f6","f7","f8","f9","f10","f11","f12","f13"], vec![]),
            ("proj-b", 5, vec![], vec!["f1","f2","f3","f4","f5","f6","f7","f8","f9","f10","f11","f12"], vec![]),
        ],
    );
    let fd = friction_density(&cluster);
    assert!(fd > 2.0);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3.indicators.iter().find(|i| i.name == "friction_density").unwrap();
    assert_eq!(fd_ind.signal, Signal::Red);
}

#[test]
fn test_layer1_has_4_indicators() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster(HashMap::new(), HashMap::new(), 0, 0, vec![("proj-a", 5, vec![])]);
    let l1 = layer1(&cluster, &t);
    assert_eq!(l1.indicators.len(), 4);
    assert_eq!(l1.indicators[3].name, "knowledge_rate");
}

#[test]
fn test_layer3_has_4_indicators() {
    use crate::config::Targets;
    let t = Targets::default();
    let cluster = make_cluster(HashMap::new(), HashMap::new(), 0, 0, vec![("proj-a", 5, vec![])]);
    let l3 = layer3(&cluster, &t);
    assert_eq!(l3.indicators.len(), 4);
    assert_eq!(l3.indicators[3].name, "friction_density");
}
