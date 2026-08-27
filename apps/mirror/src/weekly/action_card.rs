use crate::lang::t;
use crate::score::{self, Indicator, ScoreResult, Signal};
use anyhow::Result;
use refine_core::session::{ClusterResult, ProjectCluster};

pub(super) fn build_weekly_action_card(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    long_term_cluster: &ClusterResult,
    recent_cluster: &ClusterResult,
) -> Result<Option<Vec<String>>> {
    let policy = crate::advice::portfolio_policy(long_term, recent)?;
    let has_non_green = [
        &policy.long_exploration,
        &policy.long_fragmentation,
        &policy.recent_exploration,
        &policy.recent_fragmentation,
    ]
    .iter()
    .any(|indicator| indicator.signal != Signal::Green);
    if !has_non_green {
        return Ok(None);
    }

    let (candidate_window, cluster) = candidate_cohort(&policy, long_term_cluster, recent_cluster);

    let mut projects = cluster
        .projects
        .values()
        .filter(|project| project.session_count > 0)
        .filter(|project| project.project_name != "other")
        .collect::<Vec<_>>();
    projects.sort_by_key(|project| {
        (
            std::cmp::Reverse(project.session_count),
            &project.project_name,
        )
    });
    let promote = projects.first().copied().ok_or_else(|| {
        anyhow::anyhow!(
            "portfolio action card requires at least one named active project; refusing to render decisions from the 'other' bucket"
        )
    })?;

    let mut lines = Vec::new();
    lines.push(t!("## Weekly Action Card", "## 下周行动卡").to_string());
    lines.push(String::new());
    lines.push(format!("{}:", t!("Trigger", "触发原因")));
    lines.push(window_trigger("90d", &policy.long_exploration));
    lines.push(window_trigger("90d", &policy.long_fragmentation));
    lines.push(window_trigger("7d", &policy.recent_exploration));
    lines.push(window_trigger("7d", &policy.recent_fragmentation));
    lines.push(String::new());
    lines.push(format!(
        "{}: {}",
        t!("Decision cohort", "决策候选窗口"),
        candidate_window.label()
    ));
    lines.push(String::new());
    match policy.mode {
        crate::advice::PortfolioMode::PromoteHoldStop => {
            let stop = projects
                .last()
                .copied()
                .filter(|project| project.project_name != promote.project_name);
            let hold = projects.get(1).copied().filter(|project| {
                stop.is_none_or(|stop| project.project_name != stop.project_name)
                    && project.project_name != promote.project_name
            });
            lines.push(format!("{}:", t!("Portfolio decision", "项目组合决策")));
            lines.push(format!(
                "- {}",
                t!(
                    format!(
                        "Promote {}: protect the main line and verify `{}`.",
                        promote.project_name,
                        project_evidence(promote)
                    ),
                    format!(
                        "晋升 {}：保护主线，并验证「{}」。",
                        promote.project_name,
                        project_evidence(promote)
                    )
                )
            ));
            lines.push(format!(
                "- {}",
                match hold {
                    Some(project) => t!(
                        format!(
                            "Hold {}: allow one bounded validation, with no scope expansion.",
                            project.project_name
                        ),
                        format!(
                            "保留 {}：只允许一次有边界的验证，不扩大范围。",
                            project.project_name
                        )
                    ),
                    None => t!(
                        "Hold: do not add another active thread this week.".to_string(),
                        "保留：本周不增加其他活跃线程。".to_string()
                    ),
                }
            ));
            lines.push(format!("- {}", stop_decision(stop)));
            lines.push(format!(
                "- {}",
                t!(
                    "Verification: record all three decisions and reduce the active one-off set before the next score.".to_string(),
                    "验证：记录三项决定，并在下次评分前减少活跃的一次性项目。".to_string()
                )
            ));
        }
        crate::advice::PortfolioMode::Explore => {
            lines.push(format!("{}:", t!("Bounded exploration", "有边界的探索")));
            lines.push(format!(
                "- {}",
                t!(
                    format!(
                        "Inside {}, test one adjacent hypothesis from `{}` and record a keep/stop decision.",
                        promote.project_name,
                        project_evidence(promote)
                    ),
                    format!(
                        "在 {} 内，从「{}」验证一个相邻假设，并记录保留或退出决定。",
                        promote.project_name,
                        project_evidence(promote)
                    )
                )
            ));
        }
        crate::advice::PortfolioMode::Deepen => {
            lines.push(format!("{}:", t!("Deepen", "深挖")));
            lines.push(format!(
                "- {}",
                t!(
                    format!(
                        "Hold the current portfolio and turn `{}` in {} into one named validation.",
                        project_evidence(promote),
                        promote.project_name
                    ),
                    format!(
                        "保持当前组合，把 {} 中的「{}」变成一项具名验证。",
                        promote.project_name,
                        project_evidence(promote)
                    )
                )
            ));
        }
    }
    Ok(Some(lines))
}

#[derive(Clone, Copy)]
enum CandidateWindow {
    Rolling90Days,
    Rolling7Days,
}

impl CandidateWindow {
    fn label(self) -> &'static str {
        match self {
            Self::Rolling90Days => t!("rolling 90 days (event time)", "滚动 90 天（事件时间）"),
            Self::Rolling7Days => t!("rolling 7 days (event time)", "滚动 7 天（事件时间）"),
        }
    }
}

fn candidate_cohort<'a>(
    policy: &crate::advice::PortfolioPolicy,
    long_term_cluster: &'a ClusterResult,
    recent_cluster: &'a ClusterResult,
) -> (CandidateWindow, &'a ClusterResult) {
    if policy.mode == crate::advice::PortfolioMode::PromoteHoldStop
        && policy.long_fragmentation.signal != Signal::Green
    {
        (CandidateWindow::Rolling90Days, long_term_cluster)
    } else {
        (CandidateWindow::Rolling7Days, recent_cluster)
    }
}

fn stop_decision(project: Option<&ProjectCluster>) -> String {
    match project {
        Some(project) => t!(
            format!(
                "Stop {} unless `{}` produces a named result by week end.",
                project.project_name,
                project_evidence(project)
            ),
            format!(
                "退出 {}，除非「{}」在周末前产出具名结果。",
                project.project_name,
                project_evidence(project)
            )
        ),
        None => t!(
            "Stop: no separate named thread is eligible; keep additions at zero.".to_string(),
            "退出：没有可单独退出的具名线程；新增项目数保持为零。".to_string()
        ),
    }
}

fn window_trigger(window: &str, indicator: &Indicator) -> String {
    format!(
        "- {} {} {} ({})",
        window,
        score::indicator_display(&indicator.name),
        format_action_value(indicator),
        localized_signal(indicator.signal)
    )
}

fn project_evidence(project: &ProjectCluster) -> String {
    if let Some(item) = [
        &project.question_items,
        &project.progress_items,
        &project.patterns,
        &project.knowledge_gained,
        &project.architectures,
        &project.summary_excerpts,
    ]
    .into_iter()
    .find_map(|items| items.first())
    {
        return truncate_chars(item, 120);
    }

    if let Some(decision) = project.decision_titles.first() {
        return truncate_chars(&format!("decision: {}", decision), 120);
    }

    if let Some(bugfix) = project.bugfix_titles.first() {
        return truncate_chars(&format!("bugfix: {}", bugfix), 120);
    }

    t!(
        format!(
            "{} had {} sessions this week; use the 10% block to produce one concrete validation note.",
            project.project_name, project.session_count
        ),
        format!(
            "{} 本周有 {} 个 session；用 10% 时间块产出一条具体验证证据。",
            project.project_name, project.session_count
        )
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn format_action_value(indicator: &Indicator) -> String {
    match indicator.name.as_str() {
        "exploration" | "deep_invest" | "fragmentation" => {
            format!("{:.1}%", indicator.actual)
        }
        _ => indicator.display_value(),
    }
}

fn localized_signal(signal: Signal) -> &'static str {
    match signal {
        Signal::Green => t!("green", "绿灯"),
        Signal::Yellow => t!("yellow", "黄灯"),
        Signal::Red => t!("red", "红灯"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::{LayerScore, ScoreResult};
    use chrono::{DateTime, Utc};
    use refine_core::session::{DataQualityStats, GlobalStats};
    use std::collections::HashMap;

    fn indicator(name: &str, actual: f64, signal: Signal) -> Indicator {
        Indicator {
            name: name.to_string(),
            actual,
            target: String::new(),
            signal,
        }
    }

    fn score_with_breadth(indicators: Vec<Indicator>) -> ScoreResult {
        let names = ["depth", "breadth", "collaboration"];
        ScoreResult {
            layers: std::array::from_fn(|idx| LayerScore {
                name: names[idx].to_string(),
                signal: if idx == 1 { Signal::Red } else { Signal::Green },
                indicators: if idx == 1 {
                    indicators.clone()
                } else {
                    Vec::new()
                },
            }),
            tension: None,
            timestamp: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    fn project(name: &str, session_count: usize, question: Option<&str>) -> ProjectCluster {
        ProjectCluster {
            project_name: name.to_string(),
            session_count,
            doc_ids: std::collections::HashSet::new(),
            summary_excerpts: Vec::new(),
            decision_titles: Vec::new(),
            bugfix_titles: Vec::new(),
            cognitive_levels: HashMap::new(),
            collaboration_modes: HashMap::new(),
            tools: Vec::new(),
            frictions: Vec::new(),
            architectures: Vec::new(),
            knowledge_gained: Vec::new(),
            patterns: Vec::new(),
            progress_items: Vec::new(),
            question_items: question
                .map(|item| vec![item.to_string()])
                .unwrap_or_default(),
            code_artifacts: Vec::new(),
        }
    }

    fn cluster(projects: Vec<ProjectCluster>) -> ClusterResult {
        let project_ranking = projects
            .iter()
            .map(|project| (project.project_name.clone(), project.session_count))
            .collect();
        ClusterResult {
            projects: projects
                .into_iter()
                .map(|project| (project.project_name.clone(), project))
                .collect(),
            global_stats: GlobalStats {
                total_sessions: 0,
                total_decisions: 0,
                total_bugfixes: 0,
                total_summaries: 0,
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tool_frequency: HashMap::new(),
                project_ranking,
            },
            data_quality: DataQualityStats::default(),
            untagged_count: 0,
        }
    }

    #[test]
    fn regression_high_recent_exploration_and_fragmentation_uses_promote_hold_stop() {
        let long_term = score_with_breadth(vec![
            indicator("exploration", 14.4, Signal::Yellow),
            indicator("deep_invest", 19.0, Signal::Yellow),
            indicator("fragmentation", 35.0, Signal::Red),
        ]);
        let recent = score_with_breadth(vec![
            indicator("exploration", 29.2, Signal::Green),
            indicator("deep_invest", 19.0, Signal::Yellow),
            indicator("fragmentation", 48.0, Signal::Red),
        ]);
        let cluster = cluster(vec![
            project("main-work", 12, Some("stabilize weekly reports")),
            project(
                "side-tool",
                1,
                Some("test whether CLI review can be automated"),
            ),
        ]);

        let card = build_weekly_action_card(&long_term, &recent, &cluster, &cluster)
            .unwrap()
            .expect("expected action card for non-green breadth indicators")
            .join("\n");

        assert!(card.contains("Weekly Action Card"));
        assert!(card.contains("90d Exploration 14.4%"));
        assert!(card.contains("7d Exploration 29.2%"));
        assert!(card.contains("Promote main-work"));
        assert!(card.contains("Hold:"));
        assert!(card.contains("Stop side-tool"));
        assert!(card.contains("side-tool"));
        assert!(card.contains("test whether CLI review can be automated"));
        assert!(!card.to_lowercase().contains("new direction"));
    }

    #[test]
    fn test_action_card_skipped_when_breadth_is_green() {
        let score = score_with_breadth(vec![
            indicator("exploration", 16.0, Signal::Green),
            indicator("deep_invest", 30.0, Signal::Green),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let cluster = cluster(vec![project("main-work", 12, None)]);

        assert!(build_weekly_action_card(&score, &score, &cluster, &cluster)
            .unwrap()
            .is_none());
    }

    #[test]
    fn low_exploration_only_when_both_windows_low_and_fragmentation_green() {
        let long_term = score_with_breadth(vec![
            indicator("exploration", 5.2, Signal::Red),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let recent = score_with_breadth(vec![
            indicator("exploration", 8.0, Signal::Red),
            indicator("fragmentation", 6.0, Signal::Green),
        ]);
        let cluster = cluster(vec![project("active-without-evidence", 3, None)]);

        let card = match build_weekly_action_card(&long_term, &recent, &cluster, &cluster).unwrap()
        {
            Some(card) => card.join("\n"),
            None => panic!("expected fallback card for non-green breadth indicators"),
        };

        assert!(card.contains("Weekly Action Card"));
        assert!(card.contains("active-without-evidence"));
        assert!(card.contains("3 sessions this week"));
        assert!(card.contains("Bounded exploration"));
    }

    #[test]
    fn recent_green_exploration_overrides_low_long_term_exploration() {
        let long_term = score_with_breadth(vec![
            indicator("exploration", 14.4, Signal::Yellow),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let recent = score_with_breadth(vec![
            indicator("exploration", 29.2, Signal::Green),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let mut project = project("decision-only", 2, None);
        project.decision_titles = vec!["keep the release path explicit".to_string()];
        let cluster = cluster(vec![project]);

        let card = match build_weekly_action_card(&long_term, &recent, &cluster, &cluster).unwrap()
        {
            Some(card) => card.join("\n"),
            None => panic!("expected card for decision-only project evidence"),
        };

        assert!(card.contains("decision-only"));
        assert!(card.contains("decision: keep the release path explicit"));
        assert!(card.contains("Deepen"));
        assert!(!card.contains("Bounded exploration"));
    }

    #[test]
    fn fragmented_portfolio_without_named_projects_fails_closed() {
        let score = score_with_breadth(vec![
            indicator("exploration", 20.0, Signal::Green),
            indicator("fragmentation", 40.0, Signal::Red),
        ]);
        let cluster = cluster(vec![project("other", 8, None)]);
        let error = build_weekly_action_card(&score, &score, &cluster, &cluster).unwrap_err();
        assert!(error.to_string().contains("named active project"));
        assert!(error.to_string().contains("'other' bucket"));
    }

    #[test]
    fn one_named_project_is_promoted_but_never_stopped() {
        let score = score_with_breadth(vec![
            indicator("exploration", 20.0, Signal::Green),
            indicator("fragmentation", 40.0, Signal::Red),
        ]);
        let cluster = cluster(vec![project("only-project", 8, None)]);
        let card = build_weekly_action_card(&score, &score, &cluster, &cluster)
            .unwrap()
            .unwrap()
            .join("\n");
        assert!(card.contains("Promote only-project"));
        assert!(!card.contains("Stop only-project"));
        assert!(card.contains("no separate named thread is eligible"));
    }

    #[test]
    fn two_named_projects_promote_and_stop_different_projects() {
        let score = score_with_breadth(vec![
            indicator("exploration", 20.0, Signal::Green),
            indicator("fragmentation", 40.0, Signal::Red),
        ]);
        let cluster = cluster(vec![
            project("core", 8, None),
            project("side", 1, None),
            project("other", 20, None),
        ]);
        let card = build_weekly_action_card(&score, &score, &cluster, &cluster)
            .unwrap()
            .unwrap()
            .join("\n");
        assert!(card.contains("Promote core"));
        assert!(card.contains("Stop side"));
        assert!(!card.contains("Promote other"));
    }

    #[test]
    fn long_term_fragmentation_uses_long_term_candidates() {
        let long_term = score_with_breadth(vec![
            indicator("exploration", 20.0, Signal::Green),
            indicator("fragmentation", 35.0, Signal::Red),
        ]);
        let recent = score_with_breadth(vec![
            indicator("exploration", 25.0, Signal::Green),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let long_term_cluster = cluster(vec![
            project("healthy-core", 12, None),
            project("healthy-secondary", 6, None),
            project("old-one-off", 1, Some("unresolved historical experiment")),
        ]);
        let recent_cluster = cluster(vec![
            project("healthy-core", 5, None),
            project("healthy-secondary", 4, None),
        ]);

        let card =
            build_weekly_action_card(&long_term, &recent, &long_term_cluster, &recent_cluster)
                .unwrap()
                .unwrap()
                .join("\n");

        assert!(card.contains("Decision cohort: rolling 90 days (event time)"));
        assert!(card.contains("Promote healthy-core"));
        assert!(card.contains("Stop old-one-off"));
        assert!(!card.contains("Stop healthy-secondary"));
    }
}
