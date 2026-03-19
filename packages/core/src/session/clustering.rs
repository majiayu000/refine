//! 本地聚类预处理
//!
//! 将 9740 条 observation 按项目分组、去重、统计，无 LLM 调用

use crate::knowledge::{Item, ItemType};
use std::collections::{HashMap, HashSet};

const META_TAGS: &[&str] = &[
    "decision", "bugfix",
    "novice", "advanced_beginner", "competent", "proficient", "expert",
    "delegation", "pair_programming", "review", "exploration", "teaching", "deep_inquiry",
    "debugging",
];

const GENERIC_PATH_SEGMENTS: &[&str] = &[
    "users", "lifcc", "desktop", "code", "ai", "tools", "agent", "work", "life",
    "information", "mutil",
];

/// 单个项目的聚类数据
#[derive(Debug, Clone)]
pub struct ProjectCluster {
    pub project_name: String,
    pub session_count: usize,
    pub summary_excerpts: Vec<String>,
    pub decision_titles: Vec<String>,
    pub bugfix_titles: Vec<String>,
    pub cognitive_levels: HashMap<String, usize>,
    pub collaboration_modes: HashMap<String, usize>,
    pub tools: Vec<String>,
    pub frictions: Vec<String>,
    pub architectures: Vec<String>,
    pub knowledge_gained: Vec<String>,
    pub patterns: Vec<String>,
}

/// 全局聚合统计
#[derive(Debug, Clone)]
pub struct GlobalStats {
    pub total_sessions: usize,
    pub total_decisions: usize,
    pub total_bugfixes: usize,
    pub total_summaries: usize,
    pub cognitive_levels: HashMap<String, usize>,
    pub collaboration_modes: HashMap<String, usize>,
    pub tool_frequency: HashMap<String, usize>,
    pub project_ranking: Vec<(String, usize)>,
}

/// 聚类结果
#[derive(Debug)]
pub struct ClusterResult {
    pub projects: HashMap<String, ProjectCluster>,
    pub global_stats: GlobalStats,
    pub untagged_count: usize,
}

/// 主函数：从全量 observation 生成聚类结果
pub fn cluster_observations(items: &[Item]) -> ClusterResult {
    let observations: Vec<&Item> = items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
        .collect();

    let mut projects: HashMap<String, ProjectCluster> = HashMap::new();
    let mut untagged_count = 0usize;
    let mut global_cognitive: HashMap<String, usize> = HashMap::new();
    let mut global_collab: HashMap<String, usize> = HashMap::new();
    let mut global_tools: HashMap<String, usize> = HashMap::new();
    let mut all_doc_ids: HashSet<String> = HashSet::new();
    let mut total_decisions = 0usize;
    let mut total_bugfixes = 0usize;
    let mut total_summaries = 0usize;

    for item in &observations {
        let tags: Vec<&str> = item.tags().iter().map(|t| t.as_str()).collect();
        let project = extract_project_from_tags(&tags);

        let project_name = match project {
            Some(name) => name,
            None => {
                untagged_count += 1;
                "other".to_string()
            }
        };

        let cluster = projects.entry(project_name.clone()).or_insert_with(|| {
            ProjectCluster {
                project_name: project_name.clone(),
                session_count: 0,
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
            }
        });

        // 跟踪 session 数
        if let Some(doc_id) = item.document_id() {
            let id_str = doc_id.as_str().to_string();
            if all_doc_ids.insert(id_str.clone()) {
                cluster.session_count += 1;
            }
        }

        if tags.contains(&"decision") {
            cluster.decision_titles.push(item.title().to_string());
            total_decisions += 1;
        } else if tags.contains(&"bugfix") {
            cluster.bugfix_titles.push(item.title().to_string());
            total_bugfixes += 1;
        } else {
            // 结构化 summary
            total_summaries += 1;
            let content = item.content();
            let excerpt: String = content.chars().take(300).collect();
            cluster.summary_excerpts.push(format!("【{}】{}", item.title(), excerpt));

            // 提取认知水平和协作模式
            for tag in &tags {
                if is_cognitive_level(tag) {
                    *cluster.cognitive_levels.entry(tag.to_string()).or_insert(0) += 1;
                    *global_cognitive.entry(tag.to_string()).or_insert(0) += 1;
                }
                if is_collab_mode(tag) {
                    *cluster.collaboration_modes.entry(tag.to_string()).or_insert(0) += 1;
                    *global_collab.entry(tag.to_string()).or_insert(0) += 1;
                }
            }

            // 从 content 提取子维度
            for tool in extract_section_items(content, "工具") {
                *global_tools.entry(tool.clone()).or_insert(0) += 1;
                cluster.tools.push(tool);
            }
            cluster.frictions.extend(extract_section_items(content, "阻力"));
            cluster.architectures.extend(extract_section_items(content, "架构"));
            cluster.knowledge_gained.extend(extract_section_items(content, "知识"));
            cluster.patterns.extend(extract_section_items(content, "模式"));
        }
    }

    // 对每个项目去重 decision/bugfix titles
    for cluster in projects.values_mut() {
        cluster.decision_titles = dedup_titles(std::mem::take(&mut cluster.decision_titles));
        cluster.bugfix_titles = dedup_titles(std::mem::take(&mut cluster.bugfix_titles));
    }

    // 项目排名
    let mut project_ranking: Vec<(String, usize)> = projects
        .iter()
        .map(|(name, c)| (name.clone(), c.session_count))
        .collect();
    project_ranking.sort_by(|a, b| b.1.cmp(&a.1));

    ClusterResult {
        projects,
        global_stats: GlobalStats {
            total_sessions: all_doc_ids.len(),
            total_decisions,
            total_bugfixes,
            total_summaries,
            cognitive_levels: global_cognitive,
            collaboration_modes: global_collab,
            tool_frequency: global_tools,
            project_ranking,
        },
        untagged_count,
    }
}

fn extract_project_from_tags(tags: &[&str]) -> Option<String> {
    tags.iter()
        .find(|t| !META_TAGS.contains(t))
        .map(|s| normalize_project_name(s))
}

pub fn normalize_project_name(raw: &str) -> String {
    let segments: Vec<&str> = raw
        .split('-')
        .filter(|s| !s.is_empty() && !GENERIC_PATH_SEGMENTS.contains(s))
        .collect();
    match segments.len() {
        0 => raw.to_string(),
        1 => segments[0].to_string(),
        _ => segments.join("-"),
    }
}

fn dedup_titles(titles: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    titles
        .into_iter()
        .filter(|t| {
            let prefix: String = t.chars().take(15).collect::<String>().to_lowercase();
            seen.insert(prefix)
        })
        .collect()
}

fn extract_section_items(content: &str, section_name: &str) -> Vec<String> {
    let mut in_section = false;
    let mut items = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.starts_with('-') {
            in_section = trimmed.trim_end_matches(':') == section_name;
            continue;
        }
        if in_section && trimmed.starts_with("- ") {
            items.push(trimmed.trim_start_matches("- ").to_string());
        } else if in_section && !trimmed.is_empty() && !trimmed.starts_with("- ") {
            in_section = false;
        }
    }
    items
}

fn is_cognitive_level(tag: &str) -> bool {
    matches!(tag, "novice" | "advanced_beginner" | "competent" | "proficient" | "expert")
}

fn is_collab_mode(tag: &str) -> bool {
    matches!(
        tag,
        "delegation" | "pair_programming" | "review" | "exploration" | "teaching" | "deep_inquiry" | "debugging"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_project_name_extracts_meaningful_segments() {
        assert_eq!(normalize_project_name("-users-lifcc-desktop-code-ai-tools-refine"), "refine");
        assert_eq!(normalize_project_name("-users-lifcc-desktop-code-ai-gateway-litellm-rs"), "gateway-litellm-rs");
        assert_eq!(normalize_project_name("-users-lifcc-desktop-code-work-life-xhh"), "xhh");
        assert_eq!(normalize_project_name("-users-lifcc--claude-mem-observer-sessions"), "claude-mem-observer-sessions");
    }

    #[test]
    fn dedup_titles_removes_duplicates() {
        let titles = vec![
            "选择 serde_json 解析配置文件".to_string(),
            "选择 serde_json 解析响应数据".to_string(),
            "采用 SQLite 存储".to_string(),
        ];
        let result = dedup_titles(titles);
        // 前两条前 15 字符相同（"选择 serde_json 解"），去重后保留 1 条
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn extract_section_items_parses_content() {
        let content = "认知水平: proficient\n\n工具:\n- cargo\n- rustfmt\n\n阻力:\n- 编译太慢";
        assert_eq!(extract_section_items(content, "工具"), vec!["cargo", "rustfmt"]);
        assert_eq!(extract_section_items(content, "阻力"), vec!["编译太慢"]);
    }
}
