use crate::config::ensure_mirror_dir;
use crate::document_save::{save_report_to_document, SaveDocumentOptions};
use crate::lang::t;
use crate::score::{self, layer_display, Signal};
use anyhow::Result;
use refine_core::infra::{llm_with_retry, LlmClient};
use refine_core::knowledge::{DocumentRepository, Item, ItemRepository, ItemType};
use refine_core::session::{cluster_observations, format_data_quality_stats, ClusterResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

struct ProjectStat {
    name: String,
    sessions: usize,
    pct: f64,
    high_skill_pct: f64,
    delegation_pct: f64,
}

struct ProfileData {
    total_sessions: usize,
    total_projects: usize,
    project_stats: Vec<ProjectStat>,
    decision_count: usize,
    bugfix_count: usize,
    decision_bugfix_ratio: f64,
    simple_sessions: usize,
    medium_sessions: usize,
    complex_sessions: usize,
    intensive_sessions: usize,
    score_summary: String,
}

fn extract_profile_data(
    cluster: &ClusterResult,
    score_summary: &str,
    items: &[Item],
) -> ProfileData {
    let total_sessions = cluster.global_stats.total_sessions;
    let total_projects = cluster.projects.len();
    // A source document may legitimately contribute to more than one project.
    // Use the same assignment-weighted denominator as breadth scoring so these
    // project shares remain comparable and sum to 100%.
    let project_session_assignments: usize = cluster
        .projects
        .values()
        .map(|project| project.session_count)
        .sum();

    let mut stats: Vec<ProjectStat> = cluster
        .projects
        .values()
        .map(|p| {
            let cog_total: usize = p.cognitive_levels.values().sum();
            let high_skill = *p.cognitive_levels.get("expert").unwrap_or(&0)
                + *p.cognitive_levels.get("proficient").unwrap_or(&0);
            let high_skill_pct = if cog_total == 0 {
                0.0
            } else {
                high_skill as f64 / cog_total as f64 * 100.0
            };

            let collab_total: usize = p.collaboration_modes.values().sum();
            let deleg = *p.collaboration_modes.get("delegation").unwrap_or(&0);
            let delegation_pct = if collab_total == 0 {
                0.0
            } else {
                deleg as f64 / collab_total as f64 * 100.0
            };

            let pct = if project_session_assignments == 0 {
                0.0
            } else {
                p.session_count as f64 / project_session_assignments as f64 * 100.0
            };

            ProjectStat {
                name: p.project_name.clone(),
                sessions: p.session_count,
                pct,
                high_skill_pct,
                delegation_pct,
            }
        })
        .collect();
    stats.sort_by_key(|s| std::cmp::Reverse(s.sessions));
    stats.truncate(10);

    // Session complexity: count observations per document_id
    let mut doc_obs_count: HashMap<String, usize> = HashMap::new();
    let eligible_doc_ids: HashSet<&str> = cluster
        .projects
        .values()
        .flat_map(|project| project.doc_ids.iter().map(String::as_str))
        .collect();
    for item in items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
    {
        if let Some(doc_id) = item.document_id() {
            if eligible_doc_ids.contains(doc_id.as_str()) {
                *doc_obs_count
                    .entry(doc_id.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
    }

    let (mut simple, mut medium, mut complex, mut intensive) = (0, 0, 0, 0);
    for &count in doc_obs_count.values() {
        match count {
            0..=4 => simple += 1,
            5..=15 => medium += 1,
            16..=30 => complex += 1,
            _ => intensive += 1,
        }
    }

    let dec = cluster.global_stats.total_decisions;
    let bug = cluster.global_stats.total_bugfixes;
    let ratio = if bug == 0 {
        0.0
    } else {
        dec as f64 / bug as f64
    };

    ProfileData {
        total_sessions,
        total_projects,
        project_stats: stats,
        decision_count: dec,
        bugfix_count: bug,
        decision_bugfix_ratio: ratio,
        simple_sessions: simple,
        medium_sessions: medium,
        complex_sessions: complex,
        intensive_sessions: intensive,
        score_summary: score_summary.to_string(),
    }
}

const ITEM_MAX_CHARS: usize = 120;
const FACET_BUDGET_CHARS: usize = 4000;

fn dedup_top(items: &[String], limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter(|s| seen.insert(s.as_str()))
        .take(limit)
        .map(|s| {
            if s.chars().count() > ITEM_MAX_CHARS {
                s.chars().take(ITEM_MAX_CHARS).collect::<String>() + "…"
            } else {
                s.clone()
            }
        })
        .collect()
}

fn build_profile_prompt(data: &ProfileData, cluster: &ClusterResult) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Total: {} sessions across {} projects",
        data.total_sessions, data.total_projects
    ));
    lines.push(String::new());
    lines.push("Top projects by time investment:".to_string());
    for (i, p) in data.project_stats.iter().enumerate() {
        lines.push(format!(
            "{}. {}: {} sessions ({:.1}%), high-skill {:.1}%, delegation {:.1}%",
            i + 1,
            p.name,
            p.sessions,
            p.pct,
            p.high_skill_pct,
            p.delegation_pct
        ));
    }
    lines.push(String::new());
    lines.push(format!(
        "Decision style: {} decisions vs {} bugfixes (ratio {:.1}:1)",
        data.decision_count, data.bugfix_count, data.decision_bugfix_ratio
    ));
    lines.push(String::new());
    lines.push(format!(
        "Session complexity: {} simple, {} medium, {} complex, {} intensive",
        data.simple_sessions, data.medium_sessions, data.complex_sessions, data.intensive_sessions
    ));
    lines.push(String::new());
    lines.push(format!("Current signal lights: {}", data.score_summary));
    lines.push(String::new());
    lines.push(format!(
        "Cohort and data quality: {}",
        format_data_quality_stats(&cluster.data_quality)
    ));
    if cluster.data_quality.is_degraded() {
        lines.push(
            "Detached observations were excluded. Do not claim historical improvement, decline, or other cross-window trends."
                .to_string(),
        );
    }

    // Per-project facet dimensions (total budget capped to avoid LLM context overflow)
    let mut facet_chars_used: usize = 0;
    for p in &data.project_stats {
        if facet_chars_used >= FACET_BUDGET_CHARS {
            break;
        }
        if let Some(c) = cluster.projects.get(&p.name) {
            let progress = dedup_top(&c.progress_items, 5);
            let questions = dedup_top(&c.question_items, 5);
            let artifacts = dedup_top(&c.code_artifacts, 10);
            if progress.is_empty() && questions.is_empty() && artifacts.is_empty() {
                continue;
            }
            let mut block = Vec::new();
            block.push(String::new());
            block.push(format!("{}:", p.name));
            if !progress.is_empty() {
                block.push(format!("  进展: {}", progress.join(" / ")));
            }
            if !questions.is_empty() {
                block.push(format!("  问题: {}", questions.join(" / ")));
            }
            if !artifacts.is_empty() {
                block.push(format!("  代码产出: {}", artifacts.join(" / ")));
            }
            let block_str = block.join("\n");
            if facet_chars_used + block_str.len() > FACET_BUDGET_CHARS {
                break;
            }
            facet_chars_used += block_str.len();
            lines.push(block_str);
        }
    }

    lines.join("\n")
}

fn system_prompt() -> &'static str {
    t!(
        "You are a cognitive portrait artist. From a developer's AI coding session data, \
         write a narrative about who they are — their investment patterns, growth areas, \
         decision style, and blind spots. End with 2-3 reflective questions. Be specific, \
         reference actual numbers. Write in second person ('you'). No bullet points, use paragraphs.",
        "You are a cognitive portrait artist. From a developer's AI coding session data, \
         write a narrative about who they are — their investment patterns, growth areas, \
         decision style, and blind spots. End with 2-3 reflective questions. Be specific, \
         reference actual numbers. Write in second person ('you'). No bullet points, use paragraphs. \
         Use Chinese."
    )
}

fn profile_with_metadata(narrative: &str, cluster: &ClusterResult) -> String {
    format!(
        "> Cohort: linked observations excluding unattended/subagent documents\n> Data quality: {}\n\n{}",
        format_data_quality_stats(&cluster.data_quality),
        narrative,
    )
}

fn format_score_summary(score: &score::ScoreResult) -> String {
    score
        .layers
        .iter()
        .map(|l| {
            let sig = match l.signal {
                Signal::Green => "G",
                Signal::Yellow => "Y",
                Signal::Red => "R",
            };
            format!("{} {}", layer_display(&l.name), sig)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn save_profile_summary(data: &ProfileData, cluster: &ClusterResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("profile-summary.txt");
    let mut lines = Vec::new();
    lines.push(format!(
        "{} sessions, {} projects",
        data.total_sessions, data.total_projects
    ));
    for p in data.project_stats.iter().take(3) {
        lines.push(format!(
            "{}: {:.0}% sessions, {:.0}% high-skill, {:.0}% delegation",
            p.name, p.pct, p.high_skill_pct, p.delegation_pct
        ));
    }
    lines.push(format!(
        "decision:bugfix = {:.1}:1",
        data.decision_bugfix_ratio
    ));
    lines.push(format!("signals: {}", data.score_summary));
    lines.push(format!(
        "data-quality: {}",
        format_data_quality_stats(&cluster.data_quality)
    ));
    std::fs::write(&path, lines.join("\n"))?;
    Ok(())
}

pub async fn handle_profile(
    item_repo: Arc<dyn ItemRepository>,
    doc_repo: Arc<dyn DocumentRepository>,
    llm: Arc<dyn LlmClient>,
) -> Result<()> {
    let items = item_repo
        .find_all()
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if items.is_empty() {
        println!("{}", t!("No observations found.", "未找到观测数据。"));
        return Ok(());
    }

    let cluster = cluster_observations(&items);
    if cluster.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "No eligible linked interactive observations (input {}, detached {}, mode-excluded {}); refusing to generate a profile",
            cluster.data_quality.input_observations,
            cluster.data_quality.detached_observations,
            cluster.data_quality.mode_excluded_observations,
        );
    }
    let config = crate::config::load();
    let score_result = score::compute(&cluster, &config.targets);
    let score_summary = format_score_summary(&score_result);

    let data = extract_profile_data(&cluster, &score_summary, &items);
    let prompt = build_profile_prompt(&data, &cluster);

    println!(
        "{}\n",
        t!("Generating cognitive profile...", "正在生成认知画像...")
    );

    let narrative = llm_with_retry(&llm, &prompt, system_prompt())
        .await
        .map_err(|e| anyhow::anyhow!("LLM profile generation failed: {}", e))?;

    let persisted_narrative = profile_with_metadata(&narrative, &cluster);
    println!("{}", persisted_narrative);

    save_profile_summary(&data, &cluster)?;
    let doc_id = save_report_to_document(
        &doc_repo,
        &persisted_narrative,
        SaveDocumentOptions {
            source: "mirror-profile",
            title_prefix: "Mirror Profile",
            url_scheme: "mirror-profile",
            save_error_context: "Failed to save profile",
        },
    )
    .await?;
    println!("\n{} (ID: {})", t!("Profile saved", "画像已保存"), doc_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::session::{ClusterResult, DataQualityStats, GlobalStats, ProjectCluster};
    use std::collections::{HashMap, HashSet};

    fn make_cluster() -> ClusterResult {
        let mut projects = HashMap::new();
        let mut cog_a = HashMap::new();
        cog_a.insert("expert".to_string(), 5);
        cog_a.insert("proficient".to_string(), 10);
        cog_a.insert("competent".to_string(), 5);

        let mut collab_a = HashMap::new();
        collab_a.insert("delegation".to_string(), 12);
        collab_a.insert("pair_programming".to_string(), 8);

        projects.insert(
            "proj-a".to_string(),
            ProjectCluster {
                project_name: "proj-a".to_string(),
                session_count: 50,
                doc_ids: HashSet::new(),
                summary_excerpts: vec!["s1".into(); 40],
                decision_titles: vec!["d1".into(); 5],
                bugfix_titles: vec!["b1".into(); 5],
                cognitive_levels: cog_a,
                collaboration_modes: collab_a,
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

        let mut cog_b = HashMap::new();
        cog_b.insert("novice".to_string(), 8);
        cog_b.insert("competent".to_string(), 2);

        let mut collab_b = HashMap::new();
        collab_b.insert("delegation".to_string(), 3);
        collab_b.insert("exploration".to_string(), 7);

        projects.insert(
            "proj-b".to_string(),
            ProjectCluster {
                project_name: "proj-b".to_string(),
                session_count: 20,
                doc_ids: HashSet::new(),
                summary_excerpts: vec!["s2".into(); 15],
                decision_titles: vec!["d2".into(); 3],
                bugfix_titles: vec!["b2".into(); 2],
                cognitive_levels: cog_b,
                collaboration_modes: collab_b,
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

        ClusterResult {
            projects,
            global_stats: GlobalStats {
                total_sessions: 70,
                total_decisions: 100,
                total_bugfixes: 50,
                total_summaries: 400,
                cognitive_levels: {
                    let mut m = HashMap::new();
                    m.insert("expert".to_string(), 5);
                    m.insert("proficient".to_string(), 10);
                    m.insert("competent".to_string(), 7);
                    m.insert("novice".to_string(), 8);
                    m
                },
                collaboration_modes: {
                    let mut m = HashMap::new();
                    m.insert("delegation".to_string(), 15);
                    m.insert("pair_programming".to_string(), 8);
                    m.insert("exploration".to_string(), 7);
                    m
                },
                tool_frequency: HashMap::new(),
                project_ranking: vec![("proj-a".to_string(), 50), ("proj-b".to_string(), 20)],
            },
            data_quality: DataQualityStats {
                input_observations: 450,
                linked_observations: 450,
                detached_observations: 0,
                mode_excluded_observations: 0,
                eligible_observations: 450,
                cohort_identity: "sha256:test-profile".into(),
            },
            untagged_count: 0,
        }
    }

    #[test]
    fn test_extract_profile_data() {
        let cluster = make_cluster();
        let data = extract_profile_data(&cluster, "Depth G, Breadth Y, Collaboration R", &[]);

        assert_eq!(data.total_sessions, 70);
        assert_eq!(data.total_projects, 2);
        assert_eq!(data.decision_count, 100);
        assert_eq!(data.bugfix_count, 50);
        assert!((data.decision_bugfix_ratio - 2.0).abs() < f64::EPSILON);

        // proj-a should be first (50 sessions)
        assert_eq!(data.project_stats[0].name, "proj-a");
        assert_eq!(data.project_stats[0].sessions, 50);
        // expert(5) + proficient(10) = 15 out of 20 total = 75%
        assert!((data.project_stats[0].high_skill_pct - 75.0).abs() < f64::EPSILON);
        // delegation 12 out of 20 = 60%
        assert!((data.project_stats[0].delegation_pct - 60.0).abs() < f64::EPSILON);

        assert_eq!(data.project_stats[1].name, "proj-b");
    }

    #[test]
    fn project_shares_use_project_assignment_denominator() {
        let mut cluster = make_cluster();
        cluster.global_stats.total_sessions = 2;
        cluster.projects.get_mut("proj-a").unwrap().session_count = 1;
        cluster.projects.get_mut("proj-b").unwrap().session_count = 2;

        let data = extract_profile_data(&cluster, "G", &[]);
        let proj_a = data
            .project_stats
            .iter()
            .find(|project| project.name == "proj-a")
            .unwrap();
        let proj_b = data
            .project_stats
            .iter()
            .find(|project| project.name == "proj-b")
            .unwrap();

        assert!((proj_a.pct - 100.0 / 3.0).abs() < 0.001);
        assert!((proj_b.pct - 200.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn test_build_profile_prompt() {
        let cluster = make_cluster();
        let data = extract_profile_data(&cluster, "Depth G, Breadth Y", &[]);
        let prompt = build_profile_prompt(&data, &cluster);

        assert!(prompt.contains("70 sessions across 2 projects"));
        assert!(prompt.contains("proj-a"));
        assert!(prompt.contains("100 decisions vs 50 bugfixes"));
        assert!(prompt.contains("Depth G, Breadth Y"));
        assert!(prompt.contains("data quality"));
    }

    #[test]
    fn persisted_profile_exposes_cohort_and_quality() {
        let report = profile_with_metadata("profile body", &make_cluster());
        assert!(report.contains("linked observations excluding unattended/subagent"));
        assert!(report.contains("Data quality: 状态: OK"));
        assert!(report.ends_with("profile body"));
    }

    #[test]
    fn test_build_profile_prompt_includes_progress() {
        let mut cluster = make_cluster();
        cluster
            .projects
            .entry("proj-a".to_string())
            .and_modify(|p| {
                p.progress_items = vec!["step1".to_string(), "step2".to_string()];
            });

        let data = extract_profile_data(&cluster, "G", &[]);
        let prompt = build_profile_prompt(&data, &cluster);

        assert!(prompt.contains("进展:"));
        assert!(prompt.contains("step1"));
        assert!(prompt.contains("step2"));
    }

    #[test]
    fn test_build_profile_prompt_empty_progress() {
        let cluster = make_cluster();
        let data = extract_profile_data(&cluster, "G", &[]);
        let prompt = build_profile_prompt(&data, &cluster);

        // No facet sections emitted when all are empty
        assert!(!prompt.contains("进展:"));
        assert!(!prompt.contains("问题:"));
        assert!(!prompt.contains("代码产出:"));
    }

    #[test]
    fn test_code_artifacts_truncated() {
        let mut cluster = make_cluster();
        cluster
            .projects
            .entry("proj-a".to_string())
            .and_modify(|p| {
                p.code_artifacts = (0..30).map(|i| format!("artifact_{}", i)).collect();
            });

        let data = extract_profile_data(&cluster, "G", &[]);
        let prompt = build_profile_prompt(&data, &cluster);

        assert!(prompt.contains("代码产出:"));
        // At most 10 unique artifacts should appear
        let artifact_count = (0..30)
            .filter(|i| prompt.contains(&format!("artifact_{}", i)))
            .count();
        assert!(artifact_count <= 10);
    }
}
