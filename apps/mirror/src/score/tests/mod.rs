use super::*;
use chrono::{DateTime, TimeZone, Utc};

use refine_core::session::{ClusterResult, GlobalStats, ProjectCluster};
use std::collections::HashMap;

mod baseline;
mod compute;
mod paths;
mod persistence;
mod signal;
mod streak;

pub(super) fn make_cluster(
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
                doc_ids: std::collections::HashSet::new(),
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
                progress_items: Vec::new(),
                question_items: Vec::new(),
                code_artifacts: Vec::new(),
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
pub(super) fn make_cluster_with_data(
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
                doc_ids: std::collections::HashSet::new(),
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
                progress_items: Vec::new(),
                question_items: Vec::new(),
                code_artifacts: Vec::new(),
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
pub(super) fn make_score_result(
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

pub(super) fn rename_indicator(result: &mut ScoreResult, from: &str, to: &str) {
    for layer in &mut result.layers {
        for indicator in &mut layer.indicators {
            if indicator.name == from {
                indicator.name = to.to_string();
            }
        }
    }
}

pub(super) fn remove_indicator(result: &mut ScoreResult, name: &str) {
    for layer in &mut result.layers {
        layer.indicators.retain(|indicator| indicator.name != name);
    }
}

pub(super) fn convert_to_legacy_schema(result: &mut ScoreResult) {
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

pub(super) fn make_item_at(created_at: chrono::DateTime<Utc>) -> refine_core::knowledge::Item {
    use refine_core::knowledge::{ItemId, ItemType, RestoreParams};
    let now = Utc::now();
    refine_core::knowledge::Item::restore(RestoreParams {
        id: ItemId::new(),
        item_type: ItemType::Observation,
        title: "test".into(),
        summary: "".into(),
        content: "".into(),
        tags: vec![],
        source: None,
        document_id: None,
        excerpt: None,
        created_at,
        updated_at: now,
    })
    .unwrap()
}

pub(super) fn make_score_at_date(date: chrono::NaiveDate) -> ScoreResult {
    let ts = chrono::Utc.from_utc_datetime(&date.and_hms_opt(12, 0, 0).unwrap());
    ScoreResult {
        layers: std::array::from_fn(|i| {
            let names = ["depth", "breadth", "collaboration"];
            LayerScore {
                name: names[i].to_string(),
                signal: Signal::Green,
                indicators: vec![Indicator {
                    name: "test".into(),
                    actual: 1.0,
                    target: ">0".into(),
                    signal: Signal::Green,
                }],
            }
        }),
        tension: None,
        timestamp: ts,
    }
}
