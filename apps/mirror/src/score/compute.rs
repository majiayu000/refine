use chrono::Utc;
use refine_core::session::{ClusterResult, GlobalStats};

use crate::config::Targets;
use crate::lang::t;

use super::types::{worst, Indicator, LayerScore, ScoreResult, Signal};

const DECISION_KEYWORDS: &[&str] = &[
    "因为", "因", "原因", "选择", "采用", "because", "reason", "chose", "chosen", "adopted",
    "selected",
];

// ── Layer 1: Depth ──

pub(super) fn dreyfus_weighted(stats: &GlobalStats) -> f64 {
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
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
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

pub(super) fn knowledge_rate(cluster: &ClusterResult) -> f64 {
    let total_knowledge: usize = cluster
        .projects
        .values()
        .map(|p| p.knowledge_gained.len())
        .sum();
    let total_sessions = cluster.global_stats.total_sessions;
    if total_sessions == 0 {
        0.0
    } else {
        total_knowledge as f64 / total_sessions as f64
    }
}

pub(super) fn layer1(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let dw = dreyfus_weighted(&cluster.global_stats);
    let dq = decision_quality_rate(cluster);
    let dor = depth_output_ratio(&cluster.global_stats);
    let kr = knowledge_rate(cluster);

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
    let sig_kr = if kr >= t.knowledge_green {
        Signal::Green
    } else if kr >= t.knowledge_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };

    let indicators = vec![
        Indicator {
            name: "dreyfus".into(),
            actual: dw,
            target: format!(">{}", t.dreyfus_green),
            signal: sig_dw,
        },
        Indicator {
            name: "decision_quality".into(),
            actual: dq * 100.0,
            target: format!(">{}%", (t.decision_quality_green * 100.0) as u32),
            signal: sig_dq,
        },
        Indicator {
            name: "depth_output".into(),
            actual: dor * 100.0,
            target: format!(">{}%", (t.depth_output_green * 100.0) as u32),
            signal: sig_dor,
        },
        Indicator {
            name: "knowledge_rate".into(),
            actual: kr,
            target: format!(">{:.1}", t.knowledge_green),
            signal: sig_kr,
        },
    ];

    LayerScore {
        name: "depth".into(),
        signal: worst(&[sig_dw, sig_dq, sig_dor, sig_kr]),
        indicators,
    }
}

// ── Layer 2: Breadth ──

fn layer2(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let collab_total: usize = cluster.global_stats.collaboration_modes.values().sum();
    let exploration = *cluster
        .global_stats
        .collaboration_modes
        .get("exploration")
        .unwrap_or(&0);
    let exploration_rate = if collab_total == 0 {
        0.0
    } else {
        exploration as f64 / collab_total as f64
    };

    let total_projects = cluster.projects.len();
    let deep_projects = cluster
        .projects
        .values()
        .filter(|p| p.session_count >= 20)
        .count();
    let frag_projects = cluster
        .projects
        .values()
        .filter(|p| p.session_count == 1)
        .count();
    let deep_rate = if total_projects == 0 {
        0.0
    } else {
        deep_projects as f64 / total_projects as f64
    };
    let frag_rate = if total_projects == 0 {
        0.0
    } else {
        frag_projects as f64 / total_projects as f64
    };

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
        Indicator {
            name: "exploration".into(),
            actual: exploration_rate * 100.0,
            target: format!(">{}%", (t.exploration_green * 100.0) as u32),
            signal: sig_exp,
        },
        Indicator {
            name: "deep_invest".into(),
            actual: deep_rate * 100.0,
            target: format!(
                "{}-{}%",
                (t.deep_invest_green_lo * 100.0) as u32,
                (t.deep_invest_green_hi * 100.0) as u32
            ),
            signal: sig_deep,
        },
        Indicator {
            name: "fragmentation".into(),
            actual: frag_rate * 100.0,
            target: format!("<{}%", (t.fragmentation_green * 100.0) as u32),
            signal: sig_frag,
        },
    ];

    LayerScore {
        name: "breadth".into(),
        signal: worst(&[sig_exp, sig_deep, sig_frag]),
        indicators,
    }
}

// ── Layer 3: Collaboration ──

pub(super) fn friction_density(cluster: &ClusterResult) -> f64 {
    let total_frictions: usize = cluster.projects.values().map(|p| p.frictions.len()).sum();
    let total_sessions = cluster.global_stats.total_sessions;
    if total_sessions == 0 {
        0.0
    } else {
        total_frictions as f64 / total_sessions as f64
    }
}

pub(super) fn layer3(cluster: &ClusterResult, t: &Targets) -> LayerScore {
    let collab_total: usize = cluster.global_stats.collaboration_modes.values().sum();
    let delegation = *cluster
        .global_stats
        .collaboration_modes
        .get("delegation")
        .unwrap_or(&0);
    let delegation_rate = if collab_total == 0 {
        0.0
    } else {
        delegation as f64 / collab_total as f64
    };

    let mode_count = cluster
        .global_stats
        .collaboration_modes
        .values()
        .filter(|&&v| v > 0)
        .count();

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

    let fd = friction_density(cluster);
    // friction_density: lower is better
    let sig_fd = if fd < t.friction_green {
        Signal::Green
    } else if fd <= t.friction_yellow {
        Signal::Yellow
    } else {
        Signal::Red
    };

    let indicators = vec![
        Indicator {
            name: "delegation".into(),
            actual: delegation_rate * 100.0,
            target: format!("<{}%", (t.delegation_green * 100.0) as u32),
            signal: sig_del,
        },
        Indicator {
            name: "mode_diversity".into(),
            actual: mode_count as f64,
            target: format!(">={}", t.mode_diversity_green),
            signal: sig_div,
        },
        Indicator {
            name: "bug_decision".into(),
            actual: bug_dec_ratio,
            target: format!("<{}", t.bug_decision_green),
            signal: sig_bug,
        },
        Indicator {
            name: "friction_density".into(),
            actual: fd,
            target: format!("<{:.1}", t.friction_green),
            signal: sig_fd,
        },
    ];

    LayerScore {
        name: "collaboration".into(),
        signal: worst(&[sig_del, sig_div, sig_bug, sig_fd]),
        indicators,
    }
}

// ── Tension analysis ──

pub(super) fn analyze_tension(layers: &[LayerScore; 3]) -> Option<String> {
    let s = [layers[0].signal, layers[1].signal, layers[2].signal];
    match s {
        [Signal::Green, Signal::Red, _] => Some(
            t!(
                "L1+L2 tension: deep but narrowing — try an exploration session in a new direction",
                "层1绿+层2红 → 深耕但视野收窄，开一个新方向的探索 session"
            )
            .into(),
        ),
        [_, Signal::Green, Signal::Red] => Some(
            t!(
                "L2+L3 tension: exploring but over-delegating — try pair mode instead",
                "层2绿+层3红 → 探索多但 delegation 过高，探索时用 pair 模式而非委托"
            )
            .into(),
        ),
        [Signal::Red, _, Signal::Green] => Some(
            t!(
                "L1+L3 tension: smooth collaboration but no cognitive growth — challenge yourself",
                "层1红+层3绿 → 协作顺畅但认知没提升，你在舒适区，挑战更难的问题"
            )
            .into(),
        ),
        [Signal::Green, Signal::Green, Signal::Green] => Some(
            t!(
                "All green — healthy growth, consider raising your baseline",
                "全绿 → 健康成长，考虑提升基线标准"
            )
            .into(),
        ),
        [Signal::Red, Signal::Red, Signal::Red] => Some(
            t!(
                "All red — time to replan, run refine insights --prescription",
                "全红 → 需要重新规划，建议运行 refine insights --prescription"
            )
            .into(),
        ),
        _ => None,
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
