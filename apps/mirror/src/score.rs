use crate::config::{ensure_mirror_dir, mirror_dir, Targets};
use crate::lang::t;
use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use refine_core::knowledge::{Item, ItemRepository};
use refine_core::session::{cluster_observations, ClusterResult, GlobalStats};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::sync::Arc;

/// Filter items to only those created since the given date string (YYYY-MM-DD).
/// If `since` is None, returns all items unchanged.
pub fn filter_since(items: Vec<Item>, since: &Option<String>) -> Result<Vec<Item>> {
    let Some(since_str) = since.as_deref() else {
        return Ok(items);
    };
    let date = NaiveDate::parse_from_str(since_str, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("invalid --since date '{}': {}", since_str, e))?;
    let cutoff = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid date"))?
        .and_utc();
    Ok(items.into_iter().filter(|i| i.created_at() >= cutoff).collect())
}

const DECISION_KEYWORDS: &[&str] = &[
    "因为", "因", "原因", "选择", "采用",
    "because", "reason", "chose", "chosen", "adopted", "selected",
];

/// Minimum number of historical scores to activate personal baseline
const BASELINE_MIN_ENTRIES: usize = 7;

/// Sliding window size in days
const BASELINE_WINDOW_DAYS: i64 = 28;

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

impl Indicator {
    pub fn display_value(&self) -> String {
        match self.name.as_str() {
            "mode_diversity" => format!("{}", self.actual as usize),
            "bug_decision" => format!("{:.2}", self.actual),
            "dreyfus" => format!("{:.1}", self.actual),
            "knowledge_rate" | "friction_density" => format!("{:.1}", self.actual),
            _ => format!("{:.0}%", self.actual),
        }
    }
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

// ── Personal Baseline ──

/// 4-week rolling averages for each indicator
#[derive(Debug, Clone)]
pub struct PersonalBaseline {
    pub dreyfus_avg: f64,
    pub decision_quality_avg: f64,
    pub depth_output_avg: f64,
    pub exploration_avg: f64,
    pub deep_invest_avg: f64,
    pub fragmentation_avg: f64,
    pub delegation_avg: f64,
    pub mode_diversity_avg: f64,
    pub bug_decision_avg: f64,
    pub knowledge_rate_avg: f64,
    pub friction_density_avg: f64,
}

/// Extract a named indicator's actual value from a ScoreResult.
/// Returns None if the indicator is not found.
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
    let recent: Vec<&ScoreResult> = history
        .iter()
        .filter(|s| s.timestamp >= cutoff)
        .collect();

    if recent.len() < BASELINE_MIN_ENTRIES {
        return None;
    }

    let avg = |name: &str| -> f64 {
        let values: Vec<f64> = recent.iter().filter_map(|s| extract_indicator(s, name)).collect();
        let count = values.len() as f64;
        if count == 0.0 {
            return 0.0;
        }
        values.iter().sum::<f64>() / count
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

/// Indicator config for personal baseline re-judgment
struct IndicatorMeta {
    name: &'static str,
    baseline_value: f64,
    higher_is_better: bool,
}

/// Apply personal baseline to override signals on a ScoreResult (in-place).
fn apply_personal_baseline(result: &mut ScoreResult, baseline: &PersonalBaseline) {
    let metas = [
        IndicatorMeta { name: "dreyfus", baseline_value: baseline.dreyfus_avg, higher_is_better: true },
        IndicatorMeta { name: "decision_quality", baseline_value: baseline.decision_quality_avg, higher_is_better: true },
        IndicatorMeta { name: "depth_output", baseline_value: baseline.depth_output_avg, higher_is_better: true },
        IndicatorMeta { name: "exploration", baseline_value: baseline.exploration_avg, higher_is_better: true },
        IndicatorMeta { name: "deep_invest", baseline_value: baseline.deep_invest_avg, higher_is_better: true },
        IndicatorMeta { name: "fragmentation", baseline_value: baseline.fragmentation_avg, higher_is_better: false },
        IndicatorMeta { name: "delegation", baseline_value: baseline.delegation_avg, higher_is_better: false },
        IndicatorMeta { name: "mode_diversity", baseline_value: baseline.mode_diversity_avg, higher_is_better: true },
        IndicatorMeta { name: "bug_decision", baseline_value: baseline.bug_decision_avg, higher_is_better: false },
        IndicatorMeta { name: "knowledge_rate", baseline_value: baseline.knowledge_rate_avg, higher_is_better: true },
        IndicatorMeta { name: "friction_density", baseline_value: baseline.friction_density_avg, higher_is_better: false },
    ];

    for layer in &mut result.layers {
        for indicator in &mut layer.indicators {
            if let Some(meta) = metas.iter().find(|m| m.name == indicator.name) {
                indicator.signal =
                    signal_from_personal(indicator.actual, meta.baseline_value, meta.higher_is_better);
            }
        }
        // Recalculate layer signal as worst of its indicators
        let sigs: Vec<Signal> = layer.indicators.iter().map(|i| i.signal).collect();
        layer.signal = worst(&sigs);
    }

    // Recalculate tension after signal changes
    result.tension = analyze_tension(&result.layers);
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

fn knowledge_rate(cluster: &ClusterResult) -> f64 {
    let total_knowledge: usize = cluster
        .projects
        .values()
        .map(|p| p.knowledge_gained.len())
        .sum();
    let total_sessions = cluster.global_stats.total_sessions;
    if total_sessions == 0 { 0.0 } else { total_knowledge as f64 / total_sessions as f64 }
}

fn layer1(cluster: &ClusterResult, t: &Targets) -> LayerScore {
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
        Indicator { name: "dreyfus".into(), actual: dw, target: format!(">{}", t.dreyfus_green), signal: sig_dw },
        Indicator { name: "decision_quality".into(), actual: dq * 100.0, target: format!(">{}%", (t.decision_quality_green * 100.0) as u32), signal: sig_dq },
        Indicator { name: "depth_output".into(), actual: dor * 100.0, target: format!(">{}%", (t.depth_output_green * 100.0) as u32), signal: sig_dor },
        Indicator { name: "knowledge_rate".into(), actual: kr, target: format!(">{:.1}", t.knowledge_green), signal: sig_kr },
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

fn friction_density(cluster: &ClusterResult) -> f64 {
    let total_frictions: usize = cluster
        .projects
        .values()
        .map(|p| p.frictions.len())
        .sum();
    let total_sessions = cluster.global_stats.total_sessions;
    if total_sessions == 0 { 0.0 } else { total_frictions as f64 / total_sessions as f64 }
}

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
        Indicator { name: "delegation".into(), actual: delegation_rate * 100.0, target: format!("<{}%", (t.delegation_green * 100.0) as u32), signal: sig_del },
        Indicator { name: "mode_diversity".into(), actual: mode_count as f64, target: format!(">={}", t.mode_diversity_green), signal: sig_div },
        Indicator { name: "bug_decision".into(), actual: bug_dec_ratio, target: format!("<{}", t.bug_decision_green), signal: sig_bug },
        Indicator { name: "friction_density".into(), actual: fd, target: format!("<{:.1}", t.friction_green), signal: sig_fd },
    ];

    LayerScore {
        name: "collaboration".into(),
        signal: worst(&[sig_del, sig_div, sig_bug, sig_fd]),
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
        "knowledge_rate" => t!("Knowledge", "知识获取"),
        "friction_density" => t!("Friction", "摩擦密度"),
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
    let line = serde_json::to_string(result)?;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", line)?;
    drop(file);

    // Rotate: keep last 365 entries
    let content = std::fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() > 365 {
        let keep = &lines[lines.len() - 365..];
        std::fs::write(&path, keep.join("\n") + "\n")?;
    }
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

fn print_score(result: &ScoreResult, using_personal: bool) {
    println!("{}\n", t!("Mirror Cognitive Snapshot", "Mirror 认知镜像"));
    for layer in &result.layers {
        let details: Vec<String> = layer
            .indicators
            .iter()
            .map(|i| {
                let mark = if i.signal == Signal::Green { "✓" } else { "✗" };
                format!("{} {} {}", indicator_display(&i.name), i.display_value(), mark)
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
    if using_personal {
        println!("  {}", t!(
            "Baseline: personal (4-week rolling avg)",
            "基线: 个人(4周均值)"
        ));
    } else {
        println!("  {}", t!(
            "Baseline: default thresholds (insufficient data for personal baseline)",
            "基线: 默认阈值(数据不足4周)"
        ));
    }
}

// ── CLI handler ──

pub async fn handle_score(
    repo: Arc<dyn ItemRepository>,
    llm: Option<Arc<dyn refine_core::infra::LlmClient>>,
    since: Option<String>,
) -> Result<()> {
    let all_items = repo.find_all().await.map_err(|e| anyhow::anyhow!("{}", e))?;
    let items = filter_since(all_items, &since)?;
    let cluster = cluster_observations(&items);
    let config = crate::config::load();
    let mut result = compute(&cluster, &config.targets);

    // Try personal baseline: load history BEFORE persisting current score
    let history = load_recent_scores(365)?;
    let baseline = compute_personal_baseline(&history);
    let using_personal = baseline.is_some();

    if let Some(ref bl) = baseline {
        apply_personal_baseline(&mut result, bl);
    }

    persist_score(&result)?;
    print_score(&result, using_personal);

    // Data time range
    if !items.is_empty() {
        let (min_t, max_t) = items.iter().fold(
            (DateTime::<Utc>::MAX_UTC, DateTime::<Utc>::MIN_UTC),
            |(min, max), item| {
                let t = item.created_at();
                (if t < min { t } else { min }, if t > max { t } else { max })
            },
        );
        println!(
            "  {} {} ~ {}",
            t!("Data range:", "数据范围:"),
            min_t.format("%Y-%m-%d"),
            max_t.format("%Y-%m-%d"),
        );
    }

    // Check for pending ingest from growth-tracker
    let tracker_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".refine")
        .join("growth-tracker.json");
    if let Ok(content) = std::fs::read_to_string(&tracker_path) {
        if let Ok(tracker) = serde_json::from_str::<serde_json::Value>(&content) {
            let pending = tracker.get("pending_ingest").and_then(|v| v.as_u64()).unwrap_or(0);
            if pending > 3 {
                println!(
                    "  ⚠️ {} {} {}",
                    t!("There are", "有"),
                    pending,
                    t!(
                        "sessions not yet analyzed. Run: refine ingest-sessions",
                        "个 session 未分析。运行: refine ingest-sessions"
                    )
                );
            }
        }
    }

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
        // Only 5 entries — below BASELINE_MIN_ENTRIES (7)
        let history: Vec<ScoreResult> = (0..5)
            .map(|i| {
                make_score_result(
                    3.5, 60.0, 10.0, 0.5, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 0.8,
                    now - Duration::days(i),
                )
            })
            .collect();

        let baseline = compute_personal_baseline(&history);
        assert!(baseline.is_none(), "should return None with fewer than 7 entries");
    }

    #[test]
    fn test_personal_baseline_old_data_excluded() {
        let now = Utc::now();
        // 10 entries but all older than 28 days
        let history: Vec<ScoreResult> = (0..10)
            .map(|i| {
                make_score_result(
                    3.5, 60.0, 10.0, 0.5, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 0.8,
                    now - Duration::days(30 + i),
                )
            })
            .collect();

        let baseline = compute_personal_baseline(&history);
        assert!(baseline.is_none(), "should return None when all data is outside 28-day window");
    }

    #[test]
    fn test_personal_baseline_mixed_schema() {
        // Regression: old schema entries lack knowledge_rate/friction_density.
        // avg must divide by match count, not total entry count.
        let now = Utc::now();

        // 7 new-schema entries: knowledge_rate=0.6, friction_density=1.2
        let new_schema: Vec<ScoreResult> = (0..7)
            .map(|i| {
                make_score_result(3.0, 50.0, 10.0, 0.6, 20.0, 25.0, 15.0, 30.0, 4.0, 0.20, 1.2, now - Duration::days(i))
            })
            .collect();

        // 3 old-schema entries: no knowledge_rate or friction_density
        let old_schema: Vec<ScoreResult> = (7..10)
            .map(|i| ScoreResult {
                layers: [
                    LayerScore {
                        name: "depth".into(),
                        signal: Signal::Yellow,
                        indicators: vec![
                            Indicator { name: "dreyfus".into(), actual: 3.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "decision_quality".into(), actual: 50.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "depth_output".into(), actual: 10.0, target: String::new(), signal: Signal::Yellow },
                        ],
                    },
                    LayerScore {
                        name: "breadth".into(),
                        signal: Signal::Yellow,
                        indicators: vec![
                            Indicator { name: "exploration".into(), actual: 20.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "deep_invest".into(), actual: 25.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "fragmentation".into(), actual: 15.0, target: String::new(), signal: Signal::Yellow },
                        ],
                    },
                    LayerScore {
                        name: "collaboration".into(),
                        signal: Signal::Yellow,
                        indicators: vec![
                            Indicator { name: "delegation".into(), actual: 30.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "mode_diversity".into(), actual: 4.0, target: String::new(), signal: Signal::Yellow },
                            Indicator { name: "bug_decision".into(), actual: 0.20, target: String::new(), signal: Signal::Yellow },
                        ],
                    },
                ],
                tension: None,
                timestamp: now - Duration::days(i),
            })
            .collect();

        let history: Vec<ScoreResult> = new_schema.into_iter().chain(old_schema).collect();
        let baseline = compute_personal_baseline(&history);
        assert!(baseline.is_some(), "should produce baseline with 10 mixed-schema entries");
        let Some(baseline) = baseline else { return; };

        // Must be 0.6 (avg of 7 matches), not 0.42 (0.6*7/10 with old divisor bug)
        assert!((baseline.knowledge_rate_avg - 0.6).abs() < f64::EPSILON,
            "knowledge_rate_avg should be 0.6, got {}", baseline.knowledge_rate_avg);
        assert!((baseline.friction_density_avg - 1.2).abs() < f64::EPSILON,
            "friction_density_avg should be 1.2, got {}", baseline.friction_density_avg);
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
            fragmentation_avg: 15.0,  // lower is better
            delegation_avg: 30.0,     // lower is better
            mode_diversity_avg: 4.0,
            bug_decision_avg: 0.20,   // lower is better
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
        let kr_ind = l1.indicators.iter().find(|i| i.name == "knowledge_rate").unwrap();
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
        let t = Targets::default();
        let cluster = make_cluster(
            HashMap::new(), HashMap::new(), 0, 0,
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
            HashMap::new(), HashMap::new(), 0, 0,
            vec![("proj-a", 5, vec![])],
        );
        let l3 = layer3(&cluster, &t);
        assert_eq!(l3.indicators.len(), 4);
        assert_eq!(l3.indicators[3].name, "friction_density");
    }
}
