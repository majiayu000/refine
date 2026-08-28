use super::clustering::{
    cluster_observations, normalize_project_name, ClusterResult, DataQualityStats,
};
use crate::knowledge::{Item, ItemType};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const SUPPORTED_SESSION_DOCUMENT_SOURCES: &[&str] =
    &["claude-code-session", "codex-session", "remem-raw-session"];

/// A clustering result plus the exact source-validated input used to build it.
#[derive(Debug)]
pub struct SessionCohortCluster {
    pub cluster: ClusterResult,
    pub cohort_items: Vec<Item>,
}

/// Cognitive portrait statistics without cloning transcripts or retaining
/// qualitative section values. Projects are aligned with `eligible_items`.
#[derive(Debug)]
pub struct PortraitSessionCohort<'a> {
    pub eligible_items: Vec<&'a Item>,
    pub item_projects: Vec<String>,
    pub global_stats: PortraitGlobalStats,
    pub data_quality: DataQualityStats,
    pub untagged_count: usize,
}

#[derive(Debug)]
pub struct PortraitGlobalStats {
    pub total_sessions: usize,
    pub total_decisions: usize,
    pub total_bugfixes: usize,
    pub total_summaries: usize,
    pub cognitive_levels: HashMap<String, usize>,
    pub collaboration_modes: HashMap<String, usize>,
    pub project_ranking: Vec<(String, usize)>,
}

pub fn is_supported_session_document_source(source: &str) -> bool {
    SUPPORTED_SESSION_DOCUMENT_SOURCES.contains(&source)
}

/// Fail closed on observations linked to non-session documents or unknown
/// session providers. Detached observations stay in the input so the existing
/// data-quality contract can expose them.
pub fn cluster_session_observations(
    items: &[Item],
    document_sources: &HashMap<String, String>,
) -> SessionCohortCluster {
    let mut excluded_observations = 0usize;
    let cohort_items: Vec<Item> = items
        .iter()
        .filter_map(|item| match item.document_id() {
            None => Some(item.clone()),
            Some(document_id) => {
                let source = document_sources.get(document_id.as_str());
                if source.is_some_and(|source| is_supported_session_document_source(source)) {
                    Some(item.clone())
                } else {
                    excluded_observations += 1;
                    None
                }
            }
        })
        .collect();
    let mut cluster = cluster_observations(&cohort_items);
    cluster.data_quality.input_observations += excluded_observations;
    cluster.data_quality.linked_observations += excluded_observations;
    cluster.data_quality.source_excluded_observations = excluded_observations;
    debug_assert_eq!(
        cluster.data_quality.input_observations,
        cluster.data_quality.detached_observations
            + cluster.data_quality.mode_excluded_observations
            + cluster.data_quality.source_excluded_observations
            + cluster.data_quality.eligible_observations
    );

    SessionCohortCluster {
        cluster,
        cohort_items,
    }
}

/// Source-aware portrait cohort that retains only O(rows) references and
/// bounded tag/project metadata. Transcript text and section lines are never
/// cloned before the portrait's own bounded projection runs.
pub fn portrait_session_observations<'a>(
    items: &'a [Item],
    document_sources: &HashMap<String, String>,
) -> PortraitSessionCohort<'a> {
    let input_observations = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .count();
    let linked_observations = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .filter(|item| item.document_id().is_some())
        .count();
    let detached_observations = input_observations - linked_observations;
    let source_excluded_observations = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .filter_map(|item| item.document_id())
        .filter(|document_id| {
            !document_sources
                .get(document_id.as_str())
                .is_some_and(|source| is_supported_session_document_source(source))
        })
        .count();
    let excluded_document_ids: HashSet<&str> = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .filter(|item| {
            item.document_id().is_some_and(|document_id| {
                document_sources
                    .get(document_id.as_str())
                    .is_some_and(|source| is_supported_session_document_source(source))
            }) && item.tags().iter().any(|tag| {
                matches!(
                    tag.as_str(),
                    "session_mode_unattended" | "session_mode_subagent"
                )
            })
        })
        .filter_map(|item| item.document_id().map(|id| id.as_str()))
        .collect();
    let mode_excluded_observations = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .filter_map(|item| item.document_id())
        .filter(|document_id| {
            document_sources
                .get(document_id.as_str())
                .is_some_and(|source| is_supported_session_document_source(source))
                && excluded_document_ids.contains(document_id.as_str())
        })
        .count();
    let eligible_items: Vec<&Item> = items
        .iter()
        .filter(|item| item.item_type() == ItemType::Observation)
        .filter(|item| {
            item.document_id().is_some_and(|document_id| {
                document_sources
                    .get(document_id.as_str())
                    .is_some_and(|source| is_supported_session_document_source(source))
                    && !excluded_document_ids.contains(document_id.as_str())
            })
        })
        .collect();

    let mut eligible_ids: Vec<&str> = eligible_items
        .iter()
        .map(|item| item.id().as_str())
        .collect();
    eligible_ids.sort_unstable();
    let mut cohort_hasher = Sha256::new();
    for id in eligible_ids {
        cohort_hasher.update(id.as_bytes());
        cohort_hasher.update([0]);
    }
    let data_quality = DataQualityStats {
        input_observations,
        linked_observations,
        detached_observations,
        mode_excluded_observations,
        source_excluded_observations,
        eligible_observations: eligible_items.len(),
        cohort_identity: format!("sha256:{:x}", cohort_hasher.finalize()),
    };

    let mut document_projects: HashMap<&str, String> = HashMap::new();
    for item in &eligible_items {
        if let (Some(document_id), Some(project)) = (item.document_id(), project_from_tags(item)) {
            document_projects
                .entry(document_id.as_str())
                .or_insert(project);
        }
    }
    let mut item_projects = Vec::with_capacity(eligible_items.len());
    let mut project_documents: HashMap<String, HashSet<&str>> = HashMap::new();
    let mut all_documents = HashSet::new();
    let mut cognitive_levels = HashMap::new();
    let mut collaboration_modes = HashMap::new();
    let mut total_decisions = 0usize;
    let mut total_bugfixes = 0usize;
    let mut total_summaries = 0usize;
    let mut untagged_count = 0usize;
    for item in &eligible_items {
        let project = project_from_tags(item)
            .or_else(|| {
                item.document_id()
                    .and_then(|id| document_projects.get(id.as_str()).cloned())
            })
            .unwrap_or_else(|| {
                untagged_count += 1;
                "other".to_string()
            });
        if let Some(document_id) = item.document_id() {
            all_documents.insert(document_id.as_str());
            project_documents
                .entry(project.clone())
                .or_default()
                .insert(document_id.as_str());
        }
        let has_decision = item.tags().iter().any(|tag| tag.as_str() == "decision");
        let has_bugfix = item.tags().iter().any(|tag| tag.as_str() == "bugfix");
        if has_decision {
            total_decisions += 1;
        } else if has_bugfix {
            total_bugfixes += 1;
        } else {
            total_summaries += 1;
            for tag in item.tags() {
                if matches!(
                    tag.as_str(),
                    "novice" | "advanced_beginner" | "competent" | "proficient" | "expert"
                ) {
                    *cognitive_levels
                        .entry(tag.as_str().to_string())
                        .or_default() += 1;
                }
                if matches!(
                    tag.as_str(),
                    "delegation"
                        | "pair_programming"
                        | "review"
                        | "exploration"
                        | "teaching"
                        | "deep_inquiry"
                        | "debugging"
                ) {
                    *collaboration_modes
                        .entry(tag.as_str().to_string())
                        .or_default() += 1;
                }
            }
        }
        item_projects.push(project);
    }
    let mut project_ranking: Vec<(String, usize)> = project_documents
        .into_iter()
        .map(|(project, documents)| (project, documents.len()))
        .collect();
    project_ranking.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    PortraitSessionCohort {
        eligible_items,
        item_projects,
        global_stats: PortraitGlobalStats {
            total_sessions: all_documents.len(),
            total_decisions,
            total_bugfixes,
            total_summaries,
            cognitive_levels,
            collaboration_modes,
            project_ranking,
        },
        data_quality,
        untagged_count,
    }
}

const PORTRAIT_META_TAGS: &[&str] = &[
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

fn project_from_tags(item: &Item) -> Option<String> {
    item.tags()
        .iter()
        .map(|tag| tag.as_str())
        .filter(|tag| !PORTRAIT_META_TAGS.contains(tag))
        .filter_map(normalize_project_name)
        .max_by_key(String::len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{DocumentId, Item, Tag};

    fn linked_observation(id: &str) -> Item {
        let mut item = Item::new_observation(id, id);
        item.set_document_id(DocumentId::from(id));
        item
    }

    #[test]
    fn only_first_class_session_sources_enter_the_cohort() {
        let items = vec![
            linked_observation("claude"),
            linked_observation("codex"),
            linked_observation("remem"),
            linked_observation("grok-knowledge"),
            linked_observation("gemini-knowledge"),
        ];
        let sources = HashMap::from([
            ("claude".into(), "claude-code-session".into()),
            ("codex".into(), "codex-session".into()),
            ("remem".into(), "remem-raw-session".into()),
            ("grok-knowledge".into(), "grok".into()),
            ("gemini-knowledge".into(), "knowledge-document".into()),
        ]);
        let result = cluster_session_observations(&items, &sources);

        assert_eq!(result.cluster.data_quality.input_observations, 5);
        assert_eq!(result.cluster.data_quality.eligible_observations, 3);
        assert_eq!(result.cluster.data_quality.source_excluded_observations, 2);
        assert!(result.cluster.data_quality.is_degraded());
        assert_eq!(result.cohort_items.len(), 3);
    }

    #[test]
    fn portrait_cohort_matches_full_cluster_without_retaining_transcripts() {
        let mut summary = linked_observation("summary");
        summary
            .set_tags(vec![
                Tag::new("refine").unwrap(),
                Tag::new("competent").unwrap(),
                Tag::new("review").unwrap(),
            ])
            .unwrap();
        summary.set_content("工具:\n- cargo\n知识:\n- bounded projection");
        let mut decision = linked_observation("decision");
        decision.set_document_id(DocumentId::from("summary"));
        decision
            .set_tags(vec![Tag::new("decision").unwrap()])
            .unwrap();
        let mut bugfix = linked_observation("bugfix");
        bugfix
            .set_tags(vec![
                Tag::new("other-project").unwrap(),
                Tag::new("bugfix").unwrap(),
            ])
            .unwrap();
        let mut unattended = linked_observation("unattended");
        unattended
            .set_tags(vec![Tag::new("session_mode_unattended").unwrap()])
            .unwrap();
        let unsupported = linked_observation("unsupported");
        let items = vec![summary, decision, bugfix, unattended, unsupported];
        let sources = HashMap::from([
            ("summary".into(), "codex-session".into()),
            ("bugfix".into(), "claude-code-session".into()),
            ("unattended".into(), "remem-raw-session".into()),
            ("unsupported".into(), "grok-knowledge".into()),
        ]);

        let full = cluster_session_observations(&items, &sources);
        let portrait = portrait_session_observations(&items, &sources);
        assert_eq!(portrait.data_quality, full.cluster.data_quality);
        assert_eq!(
            (
                portrait.global_stats.total_sessions,
                portrait.global_stats.total_decisions,
                portrait.global_stats.total_bugfixes,
                portrait.global_stats.total_summaries,
                portrait.untagged_count,
            ),
            (
                full.cluster.global_stats.total_sessions,
                full.cluster.global_stats.total_decisions,
                full.cluster.global_stats.total_bugfixes,
                full.cluster.global_stats.total_summaries,
                full.cluster.untagged_count,
            )
        );
        assert_eq!(
            portrait.global_stats.cognitive_levels,
            full.cluster.global_stats.cognitive_levels
        );
        assert_eq!(
            portrait.global_stats.collaboration_modes,
            full.cluster.global_stats.collaboration_modes
        );
        assert_eq!(
            portrait.global_stats.project_ranking,
            full.cluster.global_stats.project_ranking
        );
        for (item, project) in portrait.eligible_items.iter().zip(&portrait.item_projects) {
            assert!(items.iter().any(|original| std::ptr::eq(*item, original)));
            assert_eq!(full.cluster.item_projects[item.id().as_str()], *project);
        }
    }
}
