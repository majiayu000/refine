use crate::lang::t;
use crate::score::{self, Indicator, LayerScore, ScoreResult, Signal};
use refine_core::session::{ClusterResult, ProjectCluster};

pub(super) fn build_weekly_action_card(
    this: &ScoreResult,
    cluster: &ClusterResult,
) -> Option<Vec<String>> {
    let breadth = this.layers.iter().find(|layer| layer.name == "breadth")?;
    let triggers = ["exploration", "deep_invest", "fragmentation"]
        .iter()
        .filter_map(|name| find_indicator(breadth, name))
        .filter(|indicator| indicator.signal != Signal::Green)
        .collect::<Vec<_>>();

    if triggers.is_empty() {
        return None;
    }

    let project = select_action_project(cluster, &triggers)?;
    let evidence = project_evidence(project)?;
    let mut lines = Vec::new();
    lines.push(t!("## Weekly Action Card", "## 下周行动卡").to_string());
    lines.push(String::new());
    lines.push(format!("{}:", t!("Trigger", "触发原因")));
    lines.extend(
        triggers
            .iter()
            .map(|indicator| action_card_trigger(indicator)),
    );
    lines.push(String::new());
    lines.push(format!("{}:", t!("Selected project", "选中的实际项目")));
    lines.push(format!(
        "- {}",
        t!(
            format!(
                "{} ({} sessions this week, {} decisions, {} bugfixes)",
                project.project_name,
                project.session_count,
                project.decision_titles.len(),
                project.bugfix_titles.len()
            ),
            format!(
                "{}（本周 {} 个 session，{} 个决策，{} 个 bugfix）",
                project.project_name,
                project.session_count,
                project.decision_titles.len(),
                project.bugfix_titles.len()
            )
        )
    ));
    lines.push(format!(
        "- {}",
        project_selection_reason(project, &triggers)
    ));
    lines.push(format!("{}:", t!("Project evidence", "项目证据")));
    lines.push(format!("- {}", evidence));
    lines.push(String::new());
    lines.push(format!("{}:", t!("Next week experiment", "下周实验")));
    lines.push(format!(
        "- {}",
        project_experiment(project, &triggers, &evidence)
    ));
    lines.push(format!(
        "- {}",
        t!(
            format!(
                "Decision: keep {} only if the validation produces concrete evidence; otherwise drop it from active work.",
                project.project_name
            ),
            format!(
                "决策：只有验证产出具体证据时才保留 {}；否则从活跃主线移除。",
                project.project_name
            )
        )
    ));
    Some(lines)
}

fn find_indicator<'a>(layer: &'a LayerScore, name: &str) -> Option<&'a Indicator> {
    layer
        .indicators
        .iter()
        .find(|indicator| indicator.name == name)
}

fn select_action_project<'a>(
    cluster: &'a ClusterResult,
    triggers: &[&Indicator],
) -> Option<&'a ProjectCluster> {
    let mut projects = cluster
        .projects
        .values()
        .filter(|project| project.session_count > 0)
        .filter(|project| project_evidence(project).is_some())
        .filter(|project| project.project_name != "other")
        .collect::<Vec<_>>();
    if projects.is_empty() {
        projects = cluster
            .projects
            .values()
            .filter(|project| project.session_count > 0)
            .filter(|project| project_evidence(project).is_some())
            .collect();
    }

    if has_trigger(triggers, "fragmentation") || has_trigger(triggers, "exploration") {
        return projects.into_iter().min_by_key(|project| {
            (
                project.session_count,
                std::cmp::Reverse(project.question_items.len() + project.progress_items.len()),
                project.project_name.clone(),
            )
        });
    }

    projects.into_iter().max_by_key(|project| {
        (
            project.session_count,
            project.decision_titles.len() + project.progress_items.len(),
            std::cmp::Reverse(project.project_name.clone()),
        )
    })
}

fn has_trigger(triggers: &[&Indicator], name: &str) -> bool {
    triggers.iter().any(|indicator| indicator.name == name)
}

fn project_selection_reason(project: &ProjectCluster, triggers: &[&Indicator]) -> String {
    if has_trigger(triggers, "fragmentation") {
        return t!(
            format!(
                "{} is a low-session thread; use it to make an explicit keep/drop decision.",
                project.project_name
            ),
            format!(
                "{} 是低会话线程；用它做一次明确的保留/放弃决策。",
                project.project_name
            )
        );
    }
    if has_trigger(triggers, "exploration") {
        return t!(
            format!(
                "{} is the least-worked current project; validate one adjacent direction from it.",
                project.project_name
            ),
            format!(
                "{} 是当前投入最少的项目；从它验证一个相邻新方向。",
                project.project_name
            )
        );
    }
    t!(
        format!(
            "{} is the main current project; deepen one unresolved question there.",
            project.project_name
        ),
        format!(
            "{} 是当前主项目；在这里深挖一个未解决问题。",
            project.project_name
        )
    )
}

fn project_experiment(project: &ProjectCluster, triggers: &[&Indicator], focus: &str) -> String {
    if has_trigger(triggers, "deep_invest") && !has_trigger(triggers, "fragmentation") {
        return t!(
            format!(
                "Spend the 10% block on {}: turn `{}` into a 90-minute deep validation.",
                project.project_name, focus
            ),
            format!(
                "把 10% 时间块投到 {}：把「{}」变成一次 90 分钟深度验证。",
                project.project_name, focus
            )
        );
    }

    t!(
        format!(
            "Spend the 10% block on {}: test one adjacent direction from `{}`.",
            project.project_name, focus
        ),
        format!(
            "把 10% 时间块投到 {}：从「{}」验证一个相邻新方向。",
            project.project_name, focus
        )
    )
}

fn project_evidence(project: &ProjectCluster) -> Option<String> {
    [
        &project.question_items,
        &project.progress_items,
        &project.patterns,
        &project.knowledge_gained,
        &project.architectures,
        &project.summary_excerpts,
    ]
    .into_iter()
    .find_map(|items| items.first())
    .map(|item| truncate_chars(item, 120))
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

fn action_card_trigger(indicator: &Indicator) -> String {
    let name = score::indicator_display(&indicator.name);
    let value = format_action_value(indicator);
    let signal = localized_signal(indicator.signal);

    match indicator.name.as_str() {
        "exploration" => t!(
            format!("- {} {} is below target ({})", name, value, signal),
            format!("- {} {} 低于目标（{}）", name, value, signal)
        ),
        "deep_invest" => t!(
            format!(
                "- {} {} is outside the target range ({})",
                name, value, signal
            ),
            format!("- {} {} 偏离目标区间（{}）", name, value, signal)
        ),
        "fragmentation" => t!(
            format!("- {} {} is above target ({})", name, value, signal),
            format!("- {} {} 高于目标（{}）", name, value, signal)
        ),
        _ => t!(
            format!("- {} {} needs attention ({})", name, value, signal),
            format!("- {} {} 需要关注（{}）", name, value, signal)
        ),
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
    use refine_core::session::GlobalStats;
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
            summary_excerpts: Vec::new(),
            decision_titles: vec!["decision".to_string()],
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
            untagged_count: 0,
        }
    }

    #[test]
    fn test_action_card_uses_actual_low_session_project_for_breadth_risk() {
        let score = score_with_breadth(vec![
            indicator("exploration", 5.2, Signal::Red),
            indicator("deep_invest", 19.0, Signal::Yellow),
            indicator("fragmentation", 26.0, Signal::Red),
        ]);
        let cluster = cluster(vec![
            project("main-work", 12, Some("stabilize weekly reports")),
            project(
                "side-tool",
                1,
                Some("test whether CLI review can be automated"),
            ),
        ]);

        let card = build_weekly_action_card(&score, &cluster)
            .expect("expected action card for non-green breadth indicators")
            .join("\n");

        assert!(card.contains("Weekly Action Card"));
        assert!(card.contains("Exploration 5.2%"));
        assert!(card.contains("side-tool"));
        assert!(card.contains("test whether CLI review can be automated"));
        assert!(card.contains("keep side-tool"));
    }

    #[test]
    fn test_action_card_skipped_when_breadth_is_green() {
        let score = score_with_breadth(vec![
            indicator("exploration", 16.0, Signal::Green),
            indicator("deep_invest", 30.0, Signal::Green),
            indicator("fragmentation", 5.0, Signal::Green),
        ]);
        let cluster = cluster(vec![project("main-work", 12, None)]);

        assert!(build_weekly_action_card(&score, &cluster).is_none());
    }

    #[test]
    fn test_action_card_skipped_without_project_evidence() {
        let score = score_with_breadth(vec![indicator("exploration", 5.2, Signal::Red)]);
        let cluster = cluster(vec![project("active-without-evidence", 3, None)]);

        assert!(build_weekly_action_card(&score, &cluster).is_none());
    }
}
