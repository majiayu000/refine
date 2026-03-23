use crate::config::ensure_mirror_dir;
use crate::document_save::{save_report_to_document, SaveDocumentOptions};
use crate::lang::t;
use crate::score::{self, layer_display, Signal};
use anyhow::Result;
use refine_core::infra::LlmClient;
use refine_core::knowledge::{DocumentRepository, Item, ItemRepository, ItemType};
use refine_core::session::{cluster_observations, ClusterResult};
use std::collections::HashMap;
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

            let pct = if total_sessions == 0 {
                0.0
            } else {
                p.session_count as f64 / total_sessions as f64 * 100.0
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
    stats.sort_by(|a, b| b.sessions.cmp(&a.sessions));
    stats.truncate(10);

    // Session complexity: count observations per document_id
    let mut doc_obs_count: HashMap<String, usize> = HashMap::new();
    for item in items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
    {
        if let Some(doc_id) = item.document_id() {
            *doc_obs_count
                .entry(doc_id.as_str().to_string())
                .or_insert(0) += 1;
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

fn build_profile_prompt(data: &ProfileData) -> String {
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

fn save_profile_summary(data: &ProfileData) -> Result<()> {
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
    let config = crate::config::load();
    let score_result = score::compute(&cluster, &config.targets);
    let score_summary = format_score_summary(&score_result);

    let data = extract_profile_data(&cluster, &score_summary, &items);
    let prompt = build_profile_prompt(&data);

    println!(
        "{}\n",
        t!("Generating cognitive profile...", "正在生成认知画像...")
    );

    let narrative = llm
        .complete(&prompt, Some(system_prompt()))
        .await
        .map_err(|e| anyhow::anyhow!("LLM profile generation failed: {}", e))?;

    println!("{}", narrative);

    save_profile_summary(&data)?;
    let doc_id = save_report_to_document(
        &doc_repo,
        &narrative,
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
    use refine_core::session::{ClusterResult, GlobalStats, ProjectCluster};
    use std::collections::HashMap;

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
    fn test_build_profile_prompt() {
        let cluster = make_cluster();
        let data = extract_profile_data(&cluster, "Depth G, Breadth Y", &[]);
        let prompt = build_profile_prompt(&data);

        assert!(prompt.contains("70 sessions across 2 projects"));
        assert!(prompt.contains("proj-a"));
        assert!(prompt.contains("100 decisions vs 50 bugfixes"));
        assert!(prompt.contains("Depth G, Breadth Y"));
    }
}
