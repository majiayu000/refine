use crate::config::{ensure_mirror_dir, mirror_dir, Targets};
use crate::lang::t;
use anyhow::Result;
use chrono::{DateTime, Utc};
use refine_core::knowledge::ItemRepository;
use refine_core::session::{cluster_observations, ClusterResult, GlobalStats};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::sync::Arc;

const DECISION_KEYWORDS: &[&str] = &[
    "因为", "因", "原因", "选择", "采用",
    "because", "reason", "chose", "chosen", "adopted", "selected",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    Green,
    Yellow,
    Red,
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
fn worst(signals: &[Signal]) -> Signal {
    if signals.contains(&Signal::Red) {
        Signal::Red
    } else if signals.contains(&Signal::Yellow) {
        Signal::Yellow
    } else {
        Signal::Green
    }
}

// ── Layer 1: Depth ──

fn dreyfus_weighted(stats: &GlobalStats) -> f64 {
    let weights: &[(&str, f64)] = &[
        ("novice", 1.0),
        ("advanced_beginner", 2.0),
        ("competent", 3.0),
        ("proficient", 4.0),
        ("expert", 5.0),
    ];
    let mut sum = 0.0;
    let mut count = 0usize;
    for (level, w) in weights {
        let n = *stats.cognitive_levels.get(*level).unwrap_or(&0);
        sum += n as f64 * w;
        count += n;
    }
    if count == 0 { 0.0 } else { sum / count as f64 }
}

fn decision_quality_rate(cluster: &ClusterResult) -> f64 {
    let total: usize = cluster
        .projects
        .values()
        .map(|p| p.decision_titles.len())
        .sum();
    if total == 0 {
        return 0.0;
    }
    let with_reason: usize = cluster
        .projects
        .values()
        .flat_map(|p| &p.decision_titles)
        .filter(|t| DECISION_KEYWORDS.iter().any(|kw| t.contains(kw)))
        .count();
    with_reason as f64 / total as f64
}

fn depth_output_ratio(stats: &GlobalStats) -> f64 {
    let deep_total = *stats.collaboration_modes.get("deep_inquiry").unwrap_or(&0);
    let deleg_total = *stats.collaboration_modes.get("delegation").unwrap_or(&0);
    if deep_total + deleg_total == 0 {
        return 0.0;
    }
    let expert_count = *stats.cognitive_levels.get("expert").unwrap_or(&0);
    let deep_expert_rate = if deep_total == 0 {
        0.0
    } else {
        expert_count as f64 / deep_total as f64
    };
    let deleg_expert_rate = if deleg_total == 0 {
        0.0
    } else {
        expert_count as f64 / deleg_total as f64
    };
    deep_expert_rate - deleg_expert_rate
}

fn layer1(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let dw = dreyfus_weighted(&cluster.global_stats);
    let dq = decision_quality_rate(cluster);
    let dor = depth_output_ratio(&cluster.global_stats);

    let sig_dw = if dw > t.dreyfus_green {
        Signal::Green
    } else if dw >= t.dreyfus_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_dq = if dq > t.decision_quality_green {
        Signal::Green
    } else if dq >= t.decision_quality_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_dor = if dor > t.depth_output_green {
        Signal::Green
    } else if dor >= t.depth_output_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };

    let indicators = vec![
        Indicator { name: "dreyfus".into(), actual: dw, target: format!(">{}", t.dreyfus_green), signal: sig_dw },
        Indicator { name: "decision_quality".into(), actual: dq * 100.0, target: format!(">{}%", (t.decision_quality_green * 100.0) as u32), signal: sig_dq },
        Indicator { name: "depth_output".into(), actual: dor * 100.0, target: format!(">{}%", (t.depth_output_green * 100.0) as u32), signal: sig_dor },
    ];

    LayerScore {
        name: "depth".into(),
        signal: worst(&[sig_dw, sig_dq, sig_dor]),
        indicators,
    }
}

// ── Layer 2: Breadth ──

fn layer2(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let collab_total: usize = cluster.global_stats.collaboration_modes.values().sum();
    let exploration = *cluster.global_stats.collaboration_modes.get("exploration").unwrap_or(&0);
    let exploration_rate = if collab_total == 0 { 0.0 } else { exploration as f64 / collab_total as f64 };

    let total_projects = cluster.projects.len();
    let deep_projects = cluster.projects.values().filter(|p| p.session_count >= 20).count();
    let frag_projects = cluster.projects.values().filter(|p| p.session_count == 1).count();
    let deep_rate = if total_projects == 0 { 0.0 } else { deep_projects as f64 / total_projects as f64 };
    let frag_rate = if total_projects == 0 { 0.0 } else { frag_projects as f64 / total_projects as f64 };

    let sig_exp = if exploration_rate > t.exploration_green {
        Signal::Green
    } else if exploration_rate >= t.exploration_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_deep = if deep_rate >= t.deep_invest_green_lo && deep_rate <= t.deep_invest_green_hi {
        Signal::Green
    } else if deep_rate >= t.deep_invest_yellow_lo && deep_rate <= t.deep_invest_yellow_hi {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_frag = if frag_rate < t.fragmentation_green {
        Signal::Green
    } else if frag_rate <= t.fragmentation_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };

    let indicators = vec![
        Indicator { name: "exploration".into(), actual: exploration_rate * 100.0, target: format!(">{}%", (t.exploration_green * 100.0) as u32), signal: sig_exp },
        Indicator { name: "deep_invest".into(), actual: deep_rate * 100.0, target: format!("{}-{}%", (t.deep_invest_green_lo * 100.0) as u32, (t.deep_invest_green_hi * 100.0) as u32), signal: sig_deep },
        Indicator { name: "fragmentation".into(), actual: frag_rate * 100.0, target: format!("<{}%", (t.fragmentation_green * 100.0) as u32), signal: sig_frag },
    ];

    LayerScore {
        name: "breadth".into(),
        signal: worst(&[sig_exp, sig_deep, sig_frag]),
        indicators,
    }
}

// ── Layer 3: Collaboration ──

fn layer3(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let collab_total: usize = cluster.global_stats.collaboration_modes.values().sum();
    let delegation = *cluster.global_stats.collaboration_modes.get("delegation").unwrap_or(&0);
    let delegation_rate = if collab_total == 0 { 0.0 } else { delegation as f64 / collab_total as f64 };

    let mode_count = cluster.global_stats.collaboration_modes.values().filter(|&&v| v > 0).count();

    let bug_dec_ratio = if cluster.global_stats.total_decisions == 0 {
        0.0
    } else {
        cluster.global_stats.total_bugfixes as f64 / cluster.global_stats.total_decisions as f64
    };

    let sig_del = if delegation_rate < t.delegation_green {
        Signal::Green
    } else if delegation_rate <= t.delegation_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_div = if mode_count >= t.mode_diversity_green {
        Signal::Green
    } else if mode_count >= t.mode_diversity_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };
    let sig_bug = if bug_dec_ratio < t.bug_decision_green {
        Signal::Green
    } else if bug_dec_ratio <= t.bug_decision_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };

    let indicators = vec![
        Indicator { name: "delegation".into(), actual: delegation_rate * 100.0, target: format!("<{}%", (t.delegation_green * 100.0) as u32), signal: sig_del },
        Indicator { name: "mode_diversity".into(), actual: mode_count as f64, target: format!(">={}", t.mode_diversity_green), signal: sig_div },
        Indicator { name: "bug_decision".into(), actual: bug_dec_ratio, target: format!("<{}", t.bug_decision_green), signal: sig_bug },
    ];

    LayerScore {
        name: "collaboration".into(),
        signal: worst(&[sig_del, sig_div, sig_bug]),
        indicators,
    }
}

// ── Tension analysis ──

fn analyze_tension(layers: &[LayerScore; 3]) -> Option<String> {
    let s = [layers[0].signal, layers[1].signal, layers[2].signal];
    match s {
        [Signal::Green, Signal::Red, _] => Some(t!(
            "L1+L2 tension: deep but narrowing — try an exploration session in a new direction",
            "层1绿+层2红 → 深耕但视野收窄，开一个新方向的探索 session"
        ).into()),
        [_, Signal::Green, Signal::Red] => Some(t!(
            "L2+L3 tension: exploring but over-delegating — try pair mode instead",
            "层2绿+层3红 → 探索多但 delegation 过高，探索时用 pair 模式而非委托"
        ).into()),
        [Signal::Red, _, Signal::Green] => Some(t!(
            "L1+L3 tension: smooth collaboration but no cognitive growth — challenge yourself",
            "层1红+层3绿 → 协作顺畅但认知没提升，你在舒适区，挑战更难的问题"
        ).into()),
        [Signal::Green, Signal::Green, Signal::Green] => Some(t!(
            "All green — healthy growth, consider raising your baseline",
            "全绿 → 健康成长，考虑提升基线标准"
        ).into()),
        [Signal::Red, Signal::Red, Signal::Red] => Some(t!(
            "All red — time to replan, run refine insights --prescription",
            "全红 → 需要重新规划，建议运行 refine insights --prescription"
        ).into()),
        _ => None,
    }
}

// ── Display helpers ──

pub fn layer_display(key: &str) -> &'static str {
    match key {
        "depth" => t!("Depth", "认知深度"),
        "breadth" => t!("Breadth", "战略广度"),
        "collaboration" => t!("Collaboration", "协作效能"),
        _ => "unknown",
    }
}

pub fn indicator_display(key: &str) -> &'static str {
    match key {
        "dreyfus" => "Dreyfus",
        "decision_quality" => t!("Decision Quality", "决策质量"),
        "depth_output" => t!("Depth Output", "深度产出比"),
        "exploration" => t!("Exploration", "探索率"),
        "deep_invest" => t!("Deep Invest", "深耕率"),
        "fragmentation" => t!("Fragmentation", "碎片化"),
        "delegation" => "delegation",
        "mode_diversity" => t!("Mode Diversity", "模式多样性"),
        "bug_decision" => "bug/decision",
        _ => "unknown",
    }
}

// ── Compute entry ──

pub fn compute(cluster: &ClusterResult, targets: &Targets) -> ScoreResult {
    let l1 = layer1(cluster, targets);
    let l2 = layer2(cluster, targets);
    let l3 = layer3(cluster, targets);
    let tension = analyze_tension(&[l1.clone(), l2.clone(), l3.clone()]);
    ScoreResult {
        layers: [l1, l2, l3],
        tension,
        timestamp: Utc::now(),
    }
}

// ── Persistence ──

pub fn persist_score(result: &ScoreResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("scores.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(result)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

pub fn load_recent_scores(n: usize) -> Result<Vec<ScoreResult>> {
    let path = mirror_dir().join("scores.jsonl");
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Ok(Vec::new()),
    };
    let reader = std::io::BufReader::new(file);
    let all: Vec<ScoreResult> = reader
        .lines()
        .map_while(|line| line.ok())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    let start = all.len().saturating_sub(n);
    Ok(all[start..].to_vec())
}

// ── Output ──

fn print_score(result: &ScoreResult) {
    println!("{}\n", t!("Mirror Cognitive Snapshot", "Mirror 认知镜像"));
    for layer in &result.layers {
        let details: Vec<String> = layer
            .indicators
            .iter()
            .map(|i| {
                let mark = if i.signal == Signal::Green { "✓" } else { "✗" };
                let name = indicator_display(&i.name);
                match i.name.as_str() {
                    "mode_diversity" => format!("{} {} {}", name, i.actual as usize, mark),
                    "bug_decision" => format!("{} {:.2} {}", name, i.actual, mark),
                    "dreyfus" => format!("{} {:.1} {}", name, i.actual, mark),
                    _ => format!("{} {:.0}% {}", name, i.actual, mark),
                }
            })
            .collect();
        println!(
            "  {:<12} {}  {}",
            layer_display(&layer.name),
            layer.signal,
            details.join(" | ")
        );
    }
    if let Some(ref tension) = result.tension {
        println!("\n  {}{}", t!("Tension: ", "张力: "), tension);
    }
    println!("  {}", t!("Baseline: default thresholds", "基线: 默认阈值"));
}

// ── CLI handler ──

pub async fn handle_score(
    repo: Arc<dyn ItemRepository>,
    llm: Option<Arc<dyn refine_core::infra::LlmClient>>,
) -> Result<()> {
    let items = repo.find_all().await.map_err(|e| anyhow::anyhow!("{}", e))?;
    let cluster = cluster_observations(&items);
    let config = crate::config::load();
    let result = compute(&cluster, &config.targets);
    persist_score(&result)?;
    print_score(&result);

    if let Some(llm) = llm {
        match crate::advice::generate_and_cache(&result, &llm).await {
            Ok(advice) => println!(
                "\n  {} {}",
                t!("Advice:", "建议:"),
                advice
            ),
            Err(e) => tracing::debug!("advice generation skipped: {}", e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::session::{ClusterResult, GlobalStats, ProjectCluster};
    use std::collections::HashMap;

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
        let tension = analyze_tension(&[green_layer.clone(), green_layer.clone(), green_layer.clone()]);
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
                LayerScore { name: "L1".into(), signal: Signal::Green, indicators: Vec::new() },
                LayerScore { name: "L2".into(), signal: Signal::Yellow, indicators: Vec::new() },
                LayerScore { name: "L3".into(), signal: Signal::Red, indicators: Vec::new() },
            ],
            tension: Some("test tension".into()),
            timestamp: Utc::now(),
        };

        // persist
        let line = serde_json::to_string(&result).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", line).unwrap();
        drop(file);

        // load
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
}
