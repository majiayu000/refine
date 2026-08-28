//! 本地聚类预处理
//!
//! 将 9740 条 observation 按项目分组、去重、统计，无 LLM 调用

use crate::knowledge::{Item, ItemType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

const META_TAGS: &[&str] = &[
    "decision",
    "bugfix",
    "novice",
    "advanced_beginner",
    "competent",
    "proficient",
    "expert",
    "delegation",
    "pair_programming",
    "review",
    "exploration",
    "teaching",
    "deep_inquiry",
    "debugging",
    "session_mode_interactive",
    "session_mode_unattended",
    "session_mode_subagent",
    "session_mode_unknown",
];

const GENERIC_PATH_SEGMENTS: &[&str] = &[
    "users",
    "lifcc",
    "desktop",
    "code",
    "ai",
    "tools",
    "tool",
    "agent",
    "work",
    "life",
    "information",
    "mutil",
];

/// 单个项目的聚类数据
#[derive(Debug, Clone)]
pub struct ProjectCluster {
    pub project_name: String,
    pub session_count: usize,
    pub doc_ids: HashSet<String>,
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
    pub progress_items: Vec<String>,
    pub question_items: Vec<String>,
    pub code_artifacts: Vec<String>,
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

/// Quality and cohort identity for one clustering input.
///
/// The analytics cohort is deliberately strict: an observation is eligible
/// only when it is linked to a source document and that document is not tagged
/// as an unattended or subagent session. Source-aware callers additionally
/// reject observations linked to non-session document sources. The terminal
/// buckets satisfy `input = detached + mode_excluded + source_excluded + eligible`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataQualityStats {
    pub input_observations: usize,
    pub linked_observations: usize,
    pub detached_observations: usize,
    pub mode_excluded_observations: usize,
    #[serde(default)]
    pub source_excluded_observations: usize,
    pub eligible_observations: usize,
    /// Stable identity of the exact eligible item set, used by checkpoints.
    pub cohort_identity: String,
}

impl DataQualityStats {
    pub fn linked_ratio(&self) -> f64 {
        if self.input_observations == 0 {
            1.0
        } else {
            self.linked_observations as f64 / self.input_observations as f64
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.detached_observations > 0 || self.source_excluded_observations > 0
    }

    pub fn status_label(&self) -> &'static str {
        if self.is_degraded() {
            "DEGRADED"
        } else {
            "OK"
        }
    }
}

/// 聚类结果
#[derive(Debug)]
pub struct ClusterResult {
    pub projects: HashMap<String, ProjectCluster>,
    /// Exact eligible observation id to project assignment used by this cluster.
    pub item_projects: HashMap<String, String>,
    pub global_stats: GlobalStats,
    pub data_quality: DataQualityStats,
    pub untagged_count: usize,
}

/// Return the exact observation cohort used by clustering and reports.
///
/// Keeping this filter public prevents source manifests from reimplementing a
/// subtly different denominator. A session is excluded as a whole when any
/// linked observation identifies it as unattended or a subagent.
pub fn eligible_observations(items: &[Item]) -> Vec<&Item> {
    let observations: Vec<&Item> = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .collect();
    let excluded_doc_ids: HashSet<String> = observations
        .iter()
        .filter(|item| {
            item.tags()
                .iter()
                .any(|tag| is_excluded_session_mode(tag.as_str()))
        })
        .filter_map(|item| item.document_id().map(|id| id.as_str().to_string()))
        .collect();

    observations
        .into_iter()
        .filter(|item| {
            item.document_id()
                .is_some_and(|id| !excluded_doc_ids.contains(id.as_str()))
        })
        .collect()
}

/// 主函数：从全量 observation 生成聚类结果
pub fn cluster_observations(items: &[Item]) -> ClusterResult {
    let observations: Vec<&Item> = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .collect();
    let excluded_doc_ids: HashSet<String> = observations
        .iter()
        .filter(|item| {
            item.tags()
                .iter()
                .any(|tag| is_excluded_session_mode(tag.as_str()))
        })
        .filter_map(|item| item.document_id().map(|id| id.as_str().to_string()))
        .collect();

    let input_observations = observations.len();
    let linked_observations = observations
        .iter()
        .filter(|item| item.document_id().is_some())
        .count();
    let detached_observations = input_observations - linked_observations;
    let mode_excluded_observations = observations
        .iter()
        .filter_map(|item| item.document_id())
        .filter(|id| excluded_doc_ids.contains(id.as_str()))
        .count();
    let eligible_observation_count = linked_observations - mode_excluded_observations;

    // Single filtering pass: compute tags once per item to avoid double allocation.
    let obs_with_tags: Vec<(&Item, Vec<&str>)> = eligible_observations(items)
        .into_iter()
        .map(|item| {
            let tags: Vec<&str> = item.tags().iter().map(|t| t.as_str()).collect();
            (item, tags)
        })
        .collect();
    debug_assert_eq!(obs_with_tags.len(), eligible_observation_count);

    let mut eligible_item_ids: Vec<&str> = obs_with_tags
        .iter()
        .map(|(item, _)| item.id().as_str())
        .collect();
    eligible_item_ids.sort_unstable();
    let mut cohort_hasher = Sha256::new();
    for id in eligible_item_ids {
        cohort_hasher.update(id.as_bytes());
        cohort_hasher.update([0]);
    }
    let data_quality = DataQualityStats {
        input_observations,
        linked_observations,
        detached_observations,
        mode_excluded_observations,
        source_excluded_observations: 0,
        eligible_observations: eligible_observation_count,
        cohort_identity: format!("sha256:{:x}", cohort_hasher.finalize()),
    };

    // Phase 0: Build doc_id → project mapping (reuses precomputed tags)
    let mut doc_project_map: HashMap<String, String> = HashMap::new();
    for (item, tags) in &obs_with_tags {
        if let Some(doc_id) = item.document_id() {
            if let Some(name) = extract_project_from_tags(tags) {
                doc_project_map
                    .entry(doc_id.as_str().to_string())
                    .or_insert(name);
            }
        }
    }

    let mut projects: HashMap<String, ProjectCluster> = HashMap::new();
    let mut item_projects: HashMap<String, String> = HashMap::new();
    let mut untagged_count = 0usize;
    let mut global_cognitive: HashMap<String, usize> = HashMap::new();
    let mut global_collab: HashMap<String, usize> = HashMap::new();
    let mut global_tools: HashMap<String, usize> = HashMap::new();
    let mut all_doc_ids: HashSet<String> = HashSet::new();
    let mut total_decisions = 0usize;
    let mut total_bugfixes = 0usize;
    let mut total_summaries = 0usize;

    for (item, tags) in &obs_with_tags {
        // Try own tags first, then inherit from session's summary item
        let project = extract_project_from_tags(tags).or_else(|| {
            item.document_id()
                .and_then(|doc_id| doc_project_map.get(doc_id.as_str()).cloned())
        });

        let project_name = match project {
            Some(name) => name,
            None => {
                untagged_count += 1;
                "other".to_string()
            }
        };
        item_projects.insert(item.id().as_str().to_string(), project_name.clone());

        let cluster = projects
            .entry(project_name.clone())
            .or_insert_with(|| ProjectCluster {
                project_name: project_name.clone(),
                session_count: 0,
                doc_ids: HashSet::new(),
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
                question_items: Vec::new(),
                code_artifacts: Vec::new(),
            });

        // Count sessions per project, while still keeping a global unique
        // session count for the overall denominator.
        if let Some(doc_id) = item.document_id() {
            let id_str = doc_id.as_str().to_string();
            all_doc_ids.insert(id_str.clone());
            if cluster.doc_ids.insert(id_str) {
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
            cluster
                .summary_excerpts
                .push(format!("【{}】{}", item.title(), excerpt));

            // 提取认知水平和协作模式
            for tag in tags {
                if is_cognitive_level(tag) {
                    *cluster.cognitive_levels.entry(tag.to_string()).or_insert(0) += 1;
                    *global_cognitive.entry(tag.to_string()).or_insert(0) += 1;
                }
                if is_collab_mode(tag) {
                    *cluster
                        .collaboration_modes
                        .entry(tag.to_string())
                        .or_insert(0) += 1;
                    *global_collab.entry(tag.to_string()).or_insert(0) += 1;
                }
            }

            // 从 content 提取子维度
            for tool in extract_section_items(content, "工具") {
                *global_tools.entry(tool.clone()).or_insert(0) += 1;
                cluster.tools.push(tool);
            }
            cluster
                .frictions
                .extend(extract_section_items(content, "阻力"));
            cluster
                .architectures
                .extend(extract_section_items(content, "架构"));
            cluster
                .knowledge_gained
                .extend(extract_section_items(content, "知识"));
            cluster
                .patterns
                .extend(extract_section_items(content, "模式"));
            cluster
                .progress_items
                .extend(extract_section_items(content, "进展"));
            cluster
                .question_items
                .extend(extract_section_items(content, "问题"));
            cluster
                .code_artifacts
                .extend(extract_section_items_capped(content, "代码产出", 20));
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
    project_ranking.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    ClusterResult {
        projects,
        item_projects,
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
        data_quality,
        untagged_count,
    }
}

fn extract_project_from_tags(tags: &[&str]) -> Option<String> {
    // Try all non-META tags, pick the one that normalizes to the most specific name
    tags.iter()
        .filter(|t| !META_TAGS.contains(t))
        .filter_map(|s| normalize_project_name(s))
        .max_by_key(|s| s.len())
}

fn is_session_id(segment: &str) -> bool {
    segment
        .strip_prefix("agent_")
        .is_some_and(|rest| rest.len() >= 16 && rest.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn normalize_project_name(raw: &str) -> Option<String> {
    let segments: Vec<&str> = raw.split('-').filter(|s| !s.is_empty()).collect();
    let first_project_segment = segments
        .iter()
        .position(|segment| !GENERIC_PATH_SEGMENTS.contains(segment))?;
    let segments: Vec<&str> = segments[first_project_segment..]
        .iter()
        .copied()
        .filter(|segment| !is_session_id(segment))
        .collect();
    match segments.len() {
        0 => None,
        1 => Some(segments[0].to_string()),
        _ => Some(segments.join("-")),
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
    extract_section_items_capped(content, section_name, usize::MAX)
}

fn extract_section_items_capped(content: &str, section_name: &str, limit: usize) -> Vec<String> {
    let mut in_section = false;
    let mut items = Vec::new();
    for line in content.lines() {
        if items.len() >= limit {
            break;
        }
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
    matches!(
        tag,
        "novice" | "advanced_beginner" | "competent" | "proficient" | "expert"
    )
}

fn is_collab_mode(tag: &str) -> bool {
    matches!(
        tag,
        "delegation"
            | "pair_programming"
            | "review"
            | "exploration"
            | "teaching"
            | "deep_inquiry"
            | "debugging"
    )
}

fn is_excluded_session_mode(tag: &str) -> bool {
    matches!(tag, "session_mode_unattended" | "session_mode_subagent")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{DocumentId, ItemId, RestoreParams, Tag};
    use chrono::Utc;

    fn observation(id: &str, doc_id: &str, tags: &[&str]) -> Item {
        observation_with_document(id, Some(doc_id), tags)
    }

    fn detached_observation(id: &str, tags: &[&str]) -> Item {
        observation_with_document(id, None, tags)
    }

    fn observation_with_document(id: &str, doc_id: Option<&str>, tags: &[&str]) -> Item {
        let now = Utc::now();
        Item::restore(RestoreParams {
            id: ItemId::from(id),
            item_type: ItemType::Observation,
            title: id.to_string(),
            summary: String::new(),
            content: "进展:\n- shipped one step\n\n问题:\n- what next?".to_string(),
            tags: tags.iter().map(|tag| Tag::new(tag).unwrap()).collect(),
            source: None,
            document_id: doc_id.map(DocumentId::from),
            excerpt: None,
            created_at: now,
            updated_at: now,
        })
        .unwrap()
    }

    #[test]
    fn normalize_project_name_extracts_meaningful_segments() {
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-ai-tools-refine"),
            Some("refine".into())
        );
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-ai-gateway-litellm-rs"),
            Some("gateway-litellm-rs".into())
        );
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-work-life-xhh"),
            Some("xhh".into())
        );
        assert_eq!(
            normalize_project_name("-users-lifcc--claude-mem-observer-sessions"),
            Some("claude-mem-observer-sessions".into())
        );
        // Pure generic paths return None
        assert_eq!(normalize_project_name("-users-lifcc-desktop-code"), None);
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-work-life"),
            None
        );
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-ai-tool-argus"),
            Some("argus".into())
        );
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-ai-tool-argus"),
            normalize_project_name("-users-lifcc-desktop-code-ai-tools-argus")
        );
        assert_eq!(
            normalize_project_name("-users-lifcc-desktop-code-ai-tools-codex-tool"),
            Some("codex-tool".into())
        );
        assert_eq!(normalize_project_name("my-tool"), Some("my-tool".into()));
        assert_eq!(
            normalize_project_name("agent_019ec96be5fe7f53a6cca93bb6201c26"),
            None
        );
        assert_eq!(
            normalize_project_name(
                "-users-lifcc-desktop-code-ai-tools-refine-agent_019ec96be5fe7f53a6cca93bb6201c26"
            ),
            Some("refine".into())
        );
        assert_eq!(
            normalize_project_name("agent_harness"),
            Some("agent_harness".into())
        );
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
        assert_eq!(
            extract_section_items(content, "工具"),
            vec!["cargo", "rustfmt"]
        );
        assert_eq!(extract_section_items(content, "阻力"), vec!["编译太慢"]);
    }

    #[test]
    fn cluster_observations_extracts_progress_items() {
        let content = "认知水平: proficient\n\n进展:\n- step1\n- step2\n\n问题:\n- 如何优化？\n\n代码产出:\n- main.rs";
        let progress = extract_section_items(content, "进展");
        let questions = extract_section_items(content, "问题");
        let artifacts = extract_section_items(content, "代码产出");

        assert_eq!(progress, vec!["step1", "step2"]);
        assert_eq!(questions, vec!["如何优化？"]);
        assert_eq!(artifacts, vec!["main.rs"]);
    }

    #[test]
    fn cluster_observations_counts_sessions_per_project() {
        let cluster = cluster_observations(&[
            observation("a1", "doc-1", &["project-a"]),
            observation("b1", "doc-1", &["project-b"]),
            observation("b2", "doc-2", &["project-b"]),
        ]);

        assert_eq!(cluster.global_stats.total_sessions, 2);
        assert_eq!(cluster.projects["project-a"].session_count, 1);
        assert_eq!(cluster.projects["project-b"].session_count, 2);
    }

    #[test]
    fn cluster_records_direct_item_projects_and_name_stable_ranking_ties() {
        let cluster = cluster_observations(&[
            observation("zeta-item", "zeta-doc", &["zeta"]),
            observation("alpha-item", "alpha-doc", &["alpha"]),
        ]);

        assert_eq!(cluster.item_projects["zeta-item"], "zeta");
        assert_eq!(cluster.item_projects["alpha-item"], "alpha");
        assert_eq!(
            cluster.global_stats.project_ranking,
            vec![("alpha".into(), 1), ("zeta".into(), 1)]
        );
    }

    #[test]
    fn cluster_observations_excludes_unattended_and_subagent_documents() {
        let cluster = cluster_observations(&[
            observation(
                "interactive",
                "doc-1",
                &["project-a", "session_mode_interactive"],
            ),
            observation("exec", "doc-2", &["project-a", "session_mode_unattended"]),
            observation("child", "doc-3", &["project-a", "session_mode_subagent"]),
            observation("legacy", "doc-4", &["project-a"]),
        ]);

        assert_eq!(cluster.global_stats.total_sessions, 2);
        assert_eq!(cluster.projects["project-a"].session_count, 2);
        assert_eq!(cluster.data_quality.input_observations, 4);
        assert_eq!(cluster.data_quality.linked_observations, 4);
        assert_eq!(cluster.data_quality.mode_excluded_observations, 2);
        assert_eq!(cluster.data_quality.eligible_observations, 2);
    }

    #[test]
    fn excluded_mode_on_one_item_excludes_the_whole_document() {
        let cluster = cluster_observations(&[
            observation("summary", "doc-1", &["project-a"]),
            observation("decision", "doc-1", &["decision", "session_mode_subagent"]),
        ]);

        assert_eq!(cluster.global_stats.total_sessions, 0);
        assert!(cluster.projects.is_empty());
    }

    #[test]
    fn detached_facets_are_excluded_from_all_analytics() {
        let cluster = cluster_observations(&[
            observation(
                "linked-summary",
                "doc-1",
                &["project-a", "session_mode_unknown"],
            ),
            observation("linked-decision", "doc-1", &["project-a", "decision"]),
            observation("linked-bug", "doc-1", &["project-a", "bugfix"]),
            detached_observation("detached-decision", &["project-b", "decision"]),
            detached_observation("detached-bug", &["project-b", "bugfix"]),
        ]);

        assert_eq!(cluster.global_stats.total_sessions, 1);
        assert_eq!(cluster.global_stats.total_decisions, 1);
        assert_eq!(cluster.global_stats.total_bugfixes, 1);
        assert_eq!(
            cluster.global_stats.project_ranking,
            vec![("project-a".into(), 1)]
        );
        assert!(!cluster.projects.contains_key("project-b"));
        assert_eq!(cluster.data_quality.input_observations, 5);
        assert_eq!(cluster.data_quality.linked_observations, 3);
        assert_eq!(cluster.data_quality.detached_observations, 2);
        assert_eq!(cluster.data_quality.mode_excluded_observations, 0);
        assert_eq!(cluster.data_quality.eligible_observations, 3);
        assert!((cluster.data_quality.linked_ratio() - 0.6).abs() < f64::EPSILON);
        assert!(cluster.data_quality.is_degraded());
    }

    #[test]
    fn strict_cohort_matches_the_documented_production_baseline_fixture() {
        const SESSIONS: usize = 3_786;
        const LINKED_DECISIONS: usize = 16_461;
        const LINKED_BUGFIXES: usize = 9_135;
        const REPORTED_DECISIONS: usize = 59_424;
        const REPORTED_BUGFIXES: usize = 28_538;

        let mut items = Vec::with_capacity(SESSIONS + REPORTED_DECISIONS + REPORTED_BUGFIXES);
        for index in 0..SESSIONS {
            items.push(observation(
                &format!("summary-{index}"),
                &format!("doc-{index}"),
                &["baseline", "session_mode_unknown"],
            ));
        }
        for index in 0..LINKED_DECISIONS {
            items.push(observation(
                &format!("linked-decision-{index}"),
                &format!("doc-{}", index % SESSIONS),
                &["baseline", "decision"],
            ));
        }
        for index in 0..LINKED_BUGFIXES {
            items.push(observation(
                &format!("linked-bugfix-{index}"),
                &format!("doc-{}", index % SESSIONS),
                &["baseline", "bugfix"],
            ));
        }
        for index in LINKED_DECISIONS..REPORTED_DECISIONS {
            items.push(detached_observation(
                &format!("detached-decision-{index}"),
                &["baseline", "decision"],
            ));
        }
        for index in LINKED_BUGFIXES..REPORTED_BUGFIXES {
            items.push(detached_observation(
                &format!("detached-bugfix-{index}"),
                &["baseline", "bugfix"],
            ));
        }

        let cluster = cluster_observations(&items);
        assert_eq!(cluster.global_stats.total_sessions, SESSIONS);
        assert_eq!(cluster.global_stats.total_decisions, LINKED_DECISIONS);
        assert_eq!(cluster.global_stats.total_bugfixes, LINKED_BUGFIXES);
        assert_eq!(
            cluster.data_quality.detached_observations,
            (REPORTED_DECISIONS - LINKED_DECISIONS) + (REPORTED_BUGFIXES - LINKED_BUGFIXES)
        );
    }
}
