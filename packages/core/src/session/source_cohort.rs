use super::clustering::{cluster_observations, ClusterResult};
use crate::knowledge::Item;
use std::collections::HashMap;

pub const SUPPORTED_SESSION_DOCUMENT_SOURCES: &[&str] =
    &["claude-code-session", "codex-session", "remem-raw-session"];

/// A clustering result plus the exact source-validated input used to build it.
#[derive(Debug)]
pub struct SessionCohortCluster {
    pub cluster: ClusterResult,
    pub cohort_items: Vec<Item>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{DocumentId, Item};

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
}
