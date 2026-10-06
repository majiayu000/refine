use chrono::Utc;
use refine_core::session::{ClusterResult, GlobalStats};
use std::collections::HashSet;

use crate::config::Targets;
use crate::lang::t;

use super::types::{worst, EvidenceCoverage, Indicator, LayerScore, ScoreResult, Signal};

// This remains a lexical observation, not a decision-quality judgment. Choice
// verbs alone do not establish a rationale; English matching is case-insensitive.
const REASON_MARKERS: &[&str] = &[
    "因为",
    "原因",
    "由于",
    "考虑到",
    "鉴于",
    "以便",
    "为了",
    "because",
    "due to",
    "rationale:",
    "rationale：",
    "reason:",
    "reason：",
    "so that",
];

fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn above(actual: Option<f64>, green: f64, yellow: f64) -> Signal {
    match actual {
        Some(value) if value > green => Signal::Green,
        Some(value) if value >= yellow => Signal::Yellow,
        Some(_) => Signal::Red,
        None => Signal::Unknown,
    }
}

fn below(actual: Option<f64>, green: f64, yellow: f64) -> Signal {
    match actual {
        Some(value) if value < green => Signal::Green,
        Some(value) if value <= yellow => Signal::Yellow,
        Some(_) => Signal::Red,
        None => Signal::Unknown,
    }
}

fn measured(
    name: &str,
    actual: Option<f64>,
    target: String,
    signal: Signal,
    coverage: EvidenceCoverage,
) -> Indicator {
    Indicator {
        name: name.into(),
        actual,
        target,
        signal,
        coverage: Some(coverage),
    }
}

fn layer(name: &str, indicators: Vec<Indicator>) -> LayerScore {
    LayerScore {
        name: name.into(),
        signal: worst(
            &indicators
                .iter()
                .map(|indicator| indicator.signal)
                .collect::<Vec<_>>(),
        ),
        indicators,
    }
}

pub(super) fn dreyfus_weighted(stats: &GlobalStats) -> Option<f64> {
    let weights = [
        ("novice", 1.0),
        ("advanced_beginner", 2.0),
        ("competent", 3.0),
        ("proficient", 4.0),
        ("expert", 5.0),
    ];
    let (sum, count) = weights
        .iter()
        .fold((0.0, 0usize), |(sum, count), (level, weight)| {
            let n = stats.cognitive_levels.get(*level).copied().unwrap_or(0);
            (sum + n as f64 * weight, count + n)
        });
    (count > 0).then(|| sum / count as f64)
}

fn reason_explicitness(cluster: &ClusterResult) -> (Option<f64>, EvidenceCoverage) {
    let titles: Vec<_> = cluster
        .projects
        .values()
        .flat_map(|project| &project.decision_titles)
        .filter(|title| !title.trim().is_empty())
        .collect();
    let with_reason = titles
        .iter()
        .filter(|title| {
            let normalized = title.to_lowercase();
            REASON_MARKERS
                .iter()
                .any(|marker| normalized.contains(marker))
        })
        .count();
    (
        ratio(with_reason, titles.len()),
        EvidenceCoverage::new(titles.len(), titles.len(), "unique_decisions"),
    )
}

pub(super) fn layer1(cluster: &ClusterResult, targets: &Targets) -> LayerScore {
    let dreyfus = dreyfus_weighted(&cluster.global_stats);
    let observed = cluster.global_stats.cognitive_levels.values().sum();
    let (reasons, reason_coverage) = reason_explicitness(cluster);
    layer(
        "depth",
        vec![
            measured(
                "dreyfus",
                dreyfus,
                format!(">{}", targets.dreyfus_green),
                above(dreyfus, targets.dreyfus_green, targets.dreyfus_yellow),
                EvidenceCoverage::new(observed, cluster.global_stats.total_summaries, "summaries"),
            ),
            measured(
                "decision_quality",
                reasons.map(|value| value * 100.0),
                format!(">{}%", (targets.decision_quality_green * 100.0) as u32),
                above(
                    reasons,
                    targets.decision_quality_green,
                    targets.decision_quality_yellow,
                ),
                reason_coverage,
            ),
        ],
    )
}

fn collaboration_coverage(cluster: &ClusterResult) -> EvidenceCoverage {
    EvidenceCoverage::new(
        cluster.global_stats.collaboration_modes.values().sum(),
        cluster.global_stats.total_summaries,
        "summaries",
    )
}

fn layer2(cluster: &ClusterResult, targets: &Targets) -> LayerScore {
    let collaboration_count = cluster.global_stats.collaboration_modes.values().sum();
    let exploration = ratio(
        cluster
            .global_stats
            .collaboration_modes
            .get("exploration")
            .copied()
            .unwrap_or(0),
        collaboration_count,
    );
    let projects: Vec<_> = cluster
        .projects
        .values()
        .filter(|project| project.project_name != "other" && project.session_count > 0)
        .collect();
    let mature = projects
        .iter()
        .filter(|project| project.session_count >= 20)
        .count();
    let one_off = projects
        .iter()
        .filter(|project| project.session_count == 1)
        .count();
    let mature_share = ratio(mature, projects.len());
    let fragmentation = ratio(one_off, projects.len());
    let assigned_sessions = projects
        .iter()
        .flat_map(|project| &project.doc_ids)
        .collect::<HashSet<_>>()
        .len();
    let project_coverage = EvidenceCoverage::new(
        assigned_sessions,
        cluster.global_stats.total_sessions,
        "sessions",
    );
    let mature_signal = match mature_share {
        Some(value)
            if value >= targets.deep_invest_green_lo && value <= targets.deep_invest_green_hi =>
        {
            Signal::Green
        }
        Some(value)
            if value >= targets.deep_invest_yellow_lo && value <= targets.deep_invest_yellow_hi =>
        {
            Signal::Yellow
        }
        Some(_) => Signal::Red,
        None => Signal::Unknown,
    };
    layer(
        "breadth",
        vec![
            measured(
                "exploration",
                exploration.map(|value| value * 100.0),
                format!(">{}%", (targets.exploration_green * 100.0) as u32),
                above(
                    exploration,
                    targets.exploration_green,
                    targets.exploration_yellow,
                ),
                collaboration_coverage(cluster),
            ),
            measured(
                "deep_invest",
                mature_share.map(|value| value * 100.0),
                format!(
                    "{}-{}%",
                    (targets.deep_invest_green_lo * 100.0) as u32,
                    (targets.deep_invest_green_hi * 100.0) as u32
                ),
                mature_signal,
                project_coverage.clone(),
            ),
            measured(
                "fragmentation",
                fragmentation.map(|value| value * 100.0),
                format!("<{}%", (targets.fragmentation_green * 100.0) as u32),
                below(
                    fragmentation,
                    targets.fragmentation_green,
                    targets.fragmentation_yellow,
                ),
                project_coverage,
            ),
        ],
    )
}

pub(super) fn layer3(cluster: &ClusterResult, targets: &Targets) -> LayerScore {
    let collaboration_count = cluster.global_stats.collaboration_modes.values().sum();
    let delegation = ratio(
        cluster
            .global_stats
            .collaboration_modes
            .get("delegation")
            .copied()
            .unwrap_or(0),
        collaboration_count,
    );
    let mode_count = (collaboration_count > 0).then(|| {
        cluster
            .global_stats
            .collaboration_modes
            .values()
            .filter(|&&count| count > 0)
            .count()
    });
    let diversity_signal = match mode_count {
        Some(value) if value >= targets.mode_diversity_green => Signal::Green,
        Some(value) if value >= targets.mode_diversity_yellow => Signal::Yellow,
        Some(_) => Signal::Red,
        None => Signal::Unknown,
    };
    let bug_ratio = ratio(
        cluster.global_stats.total_bugfixes,
        cluster.global_stats.total_decisions,
    );
    layer(
        "collaboration",
        vec![
            measured(
                "delegation",
                delegation.map(|value| value * 100.0),
                format!("<{}%", (targets.delegation_green * 100.0) as u32),
                below(
                    delegation,
                    targets.delegation_green,
                    targets.delegation_yellow,
                ),
                collaboration_coverage(cluster),
            ),
            measured(
                "mode_diversity",
                mode_count.map(|value| value as f64),
                format!(">={}", targets.mode_diversity_green),
                diversity_signal,
                collaboration_coverage(cluster),
            ),
            measured(
                "bug_decision",
                bug_ratio,
                format!("<{}", targets.bug_decision_green),
                below(
                    bug_ratio,
                    targets.bug_decision_green,
                    targets.bug_decision_yellow,
                ),
                EvidenceCoverage::new(
                    cluster.global_stats.total_decisions,
                    cluster.global_stats.total_decisions,
                    "decisions",
                ),
            ),
        ],
    )
}

// Tension describes the snapshot only. Actions belong exclusively to the
// shared 90d/7d portfolio policy, so a fragmentation warning cannot also
// instruct the user to open a new project through this display path.
pub(super) fn analyze_tension(layers: &[LayerScore; 3]) -> Option<String> {
    let signals = [layers[0].signal, layers[1].signal, layers[2].signal];
    if signals.contains(&Signal::Unknown) {
        return Some(
            t!(
                "Some metrics lack evidence; no conclusion is drawn for those metrics.",
                "部分指标证据不足，未对这些指标作行为判断。"
            )
            .into(),
        );
    }
    match signals {
        [Signal::Green, Signal::Red, _] => Some(t!(
            "Depth meets configured targets; breadth has a below-target indicator.",
            "认知深度指标达标；战略广度存在未达标指标。").into()),
        [_, Signal::Green, Signal::Red] => Some(t!(
            "Breadth meets configured targets; collaboration has a below-target indicator.",
            "战略广度指标达标；协作效能存在未达标指标。").into()),
        [Signal::Red, _, Signal::Green] => Some(t!(
            "Collaboration meets configured targets; depth has a below-target indicator.",
            "协作效能指标达标；认知深度存在未达标指标。").into()),
        [Signal::Green, Signal::Green, Signal::Green] => Some(t!(
            "All green: all three layers meet configured targets in this snapshot.",
            "当前快照中，三个层级均达到配置目标。").into()),
        [Signal::Red, Signal::Red, Signal::Red] => Some(t!(
            "All three layers include below-target indicators; inspect their evidence before choosing an action.",
            "三个层级都有未达标指标，请结合各指标证据评估。").into()),
        _ => None,
    }
}

pub fn compute(cluster: &ClusterResult, targets: &Targets) -> ScoreResult {
    let layers = [
        layer1(cluster, targets),
        layer2(cluster, targets),
        layer3(cluster, targets),
    ];
    let tension = analyze_tension(&layers);
    ScoreResult {
        layers,
        tension,
        timestamp: Utc::now(),
        scope: None,
    }
}
