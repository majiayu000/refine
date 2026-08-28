use super::clustering::{
    eligible_observations, is_generic_project_path_segment, is_project_meta_tag, is_session_id,
    normalize_project_name,
};
use crate::knowledge::Item;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
enum AliasResolution {
    Canonical(String),
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProjectResolution {
    Canonical(String),
    AmbiguousAlias(String),
}

#[derive(Debug, Clone)]
struct PathCandidate {
    full_path: Option<String>,
    display_alias: Option<String>,
    alias_keys: BTreeSet<String>,
}

/// Snapshot-level, collision-aware project identity resolver.
///
/// Build one resolver over every window that will be compared, then pass it to
/// `cluster_observations_with_resolver` for each window. This keeps a short
/// alias from resolving differently merely because one path is absent in a
/// particular window.
#[derive(Debug, Clone, Default)]
pub struct ProjectIdentityResolver {
    canonical_paths: HashMap<String, String>,
    aliases: HashMap<String, AliasResolution>,
}

impl ProjectIdentityResolver {
    pub fn from_observation_windows(windows: &[&[Item]]) -> Self {
        Self::from_eligible_items(
            windows
                .iter()
                .flat_map(|window| eligible_observations(window)),
        )
    }

    pub(super) fn from_eligible_items<'a>(items: impl IntoIterator<Item = &'a Item>) -> Self {
        let mut paths = BTreeMap::new();
        let mut bare_aliases = BTreeSet::new();
        for item in items {
            for tag in item.tags() {
                let raw = tag.as_str();
                if is_project_meta_tag(raw) {
                    continue;
                }
                if let Some(candidate) = path_candidate(raw) {
                    let Some(full_path) = candidate.full_path.clone() else {
                        continue;
                    };
                    paths
                        .entry(full_path)
                        .and_modify(|existing: &mut PathCandidate| {
                            existing.alias_keys.extend(candidate.alias_keys.clone());
                        })
                        .or_insert(candidate);
                } else if let Some(alias) = normalize_project_name(raw) {
                    bare_aliases.insert(alias);
                }
            }
        }

        let mut alias_paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (full_path, path) in &paths {
            for alias in &path.alias_keys {
                alias_paths
                    .entry(alias.clone())
                    .or_default()
                    .insert(full_path.clone());
            }
        }

        let canonical_paths: HashMap<String, String> = paths
            .iter()
            .map(|(full_path, path)| {
                let has_collision = path.alias_keys.iter().any(|alias| {
                    alias_paths
                        .get(alias)
                        .is_some_and(|path_keys| path_keys.len() > 1)
                });
                let canonical = if has_collision {
                    path_qualified_identity(full_path)
                } else {
                    path.display_alias
                        .clone()
                        .unwrap_or_else(|| path_qualified_identity(full_path))
                };
                (full_path.clone(), canonical)
            })
            .collect();

        let aliases = alias_paths
            .into_iter()
            .map(|(alias, path_keys)| {
                let resolution = if path_keys.len() != 1 {
                    AliasResolution::Ambiguous
                } else {
                    let path_key = path_keys.iter().next();
                    let is_explicit_display_alias = path_key
                        .and_then(|path_key| paths.get(path_key))
                        .and_then(|path| path.display_alias.as_deref())
                        == Some(alias.as_str());
                    if bare_aliases.contains(&alias) && !is_explicit_display_alias {
                        AliasResolution::Ambiguous
                    } else {
                        path_key
                            .and_then(|path_key| canonical_paths.get(path_key))
                            .cloned()
                            .map(AliasResolution::Canonical)
                            .unwrap_or(AliasResolution::Ambiguous)
                    }
                };
                (alias, resolution)
            })
            .collect();

        Self {
            canonical_paths,
            aliases,
        }
    }

    pub(super) fn resolve_tags(&self, tags: &[&str]) -> Option<ProjectResolution> {
        let mut canonical_paths = Vec::new();
        let mut canonical_aliases = Vec::new();
        let mut ambiguous_aliases = Vec::new();

        for raw in tags.iter().copied().filter(|tag| !is_project_meta_tag(tag)) {
            if let Some(path) = path_candidate(raw) {
                if let Some(full_path) = path.full_path {
                    canonical_paths.push(
                        self.canonical_paths
                            .get(&full_path)
                            .cloned()
                            .unwrap_or_else(|| path_qualified_identity(&full_path)),
                    );
                } else {
                    for alias in path.alias_keys {
                        match self.aliases.get(&alias) {
                            Some(AliasResolution::Canonical(canonical)) => {
                                canonical_aliases.push(canonical.clone());
                            }
                            Some(AliasResolution::Ambiguous) => ambiguous_aliases.push(alias),
                            None => canonical_aliases.push(alias),
                        }
                    }
                }
                continue;
            }
            let Some(alias) = normalize_project_name(raw) else {
                continue;
            };
            match self.aliases.get(&alias) {
                Some(AliasResolution::Canonical(canonical)) => {
                    canonical_aliases.push(canonical.clone());
                }
                Some(AliasResolution::Ambiguous) => ambiguous_aliases.push(alias),
                None => canonical_aliases.push(alias),
            }
        }

        canonical_paths
            .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        canonical_aliases
            .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        canonical_paths
            .into_iter()
            .next()
            .or_else(|| canonical_aliases.into_iter().next())
            .map(ProjectResolution::Canonical)
            .or_else(|| {
                ambiguous_aliases
                    .into_iter()
                    .min()
                    .map(ProjectResolution::AmbiguousAlias)
            })
    }
}

fn path_candidate(raw: &str) -> Option<PathCandidate> {
    let mut alias_keys = BTreeSet::new();
    if is_encoded_path(raw) {
        if let Some(alias) = normalize_encoded_path_alias(raw) {
            alias_keys.insert(alias);
        }
        return Some(PathCandidate {
            full_path: None,
            display_alias: None,
            alias_keys,
        });
    }

    let full_path = normalized_full_path(raw)?;
    let display_alias = path_leaf(raw).and_then(normalize_project_name);
    if let Some(alias) = &display_alias {
        alias_keys.insert(alias.clone());
    }
    if let Some(encoded_alias) = normalize_encoded_path_alias(&legacy_encoded_path(raw)) {
        alias_keys.insert(encoded_alias);
    }
    Some(PathCandidate {
        full_path: Some(full_path),
        display_alias,
        alias_keys,
    })
}

fn legacy_encoded_path(raw: &str) -> String {
    raw.trim()
        .trim_end_matches(['/', '\\'])
        .chars()
        .map(|character| match character {
            '/' | '\\' | '.' | ':' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Normalize only lossless path syntax. Claude's encoded cwd is deliberately
/// not treated as full-path evidence because dots, hyphens, colons, and path
/// separators have already been collapsed in that representation.
fn normalized_full_path(raw: &str) -> Option<String> {
    if !looks_like_path(raw) || is_encoded_path(raw) {
        return None;
    }
    let mut full_path = raw.trim().trim_end_matches(['/', '\\']).replace('\\', "/");
    if let Some(index) = full_path.rfind("-agent_") {
        if is_session_id(&full_path[index + 1..]) {
            full_path.truncate(index);
        }
    }
    if full_path.is_empty() {
        None
    } else {
        Some(full_path)
    }
}

fn looks_like_path(raw: &str) -> bool {
    raw.starts_with('-')
        || raw.contains('/')
        || raw.contains('\\')
        || raw.as_bytes().get(1).is_some_and(|byte| *byte == b':')
}

fn is_encoded_path(raw: &str) -> bool {
    raw.starts_with('-') && !raw.contains('/') && !raw.contains('\\')
}

fn path_leaf(raw: &str) -> Option<&str> {
    raw.trim()
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty() && !is_session_id(segment))
}

fn normalize_encoded_path_alias(raw: &str) -> Option<String> {
    let segments: Vec<&str> = raw
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect();
    let last_generic = segments
        .iter()
        .enumerate()
        .rfind(|(index, segment)| *index + 1 < segments.len() && is_legacy_generic_segment(segment))
        .map(|(index, _)| index);
    let leaf = last_generic
        .and_then(|index| segments.get(index + 1..))
        .filter(|segments| !segments.is_empty())
        .map(|segments| segments.join("-"))
        .unwrap_or_else(|| segments.join("-"));
    normalize_project_name(&leaf)
}

fn is_legacy_generic_segment(segment: &str) -> bool {
    is_generic_project_path_segment(segment) || matches!(segment, "infra" | "gateway")
}

fn path_qualified_identity(full_path: &str) -> String {
    format!("path:{full_path}")
}

#[cfg(test)]
mod tests {
    use super::super::clustering::cluster_observations_with_resolver;
    use super::*;
    use crate::knowledge::{DocumentId, ItemId, ItemType, RestoreParams, Tag};
    use chrono::Utc;

    fn observation(id: &str, project: &str) -> Item {
        let now = Utc::now();
        Item::restore(RestoreParams {
            id: ItemId::from(id),
            item_type: ItemType::Observation,
            title: id.to_string(),
            summary: String::new(),
            content: "进展:\n- shipped".to_string(),
            tags: vec![Tag::new(project).unwrap()],
            source: None,
            document_id: Some(DocumentId::from(format!("doc-{id}").as_str())),
            excerpt: None,
            created_at: now,
            updated_at: now,
        })
        .unwrap()
    }

    #[test]
    fn unix_windows_and_encoded_paths_share_legacy_alias_semantics() {
        assert_eq!(
            path_candidate("/any/home/work/mutil-om").map(|candidate| candidate.alias_keys),
            Some(BTreeSet::from(["om".into()]))
        );
        assert_eq!(
            path_candidate("-any-home-work-mutil-om").map(|candidate| candidate.alias_keys),
            Some(BTreeSet::from(["om".into()]))
        );
        assert_eq!(
            path_candidate(r"C:\any\home\work\mutil-om").map(|candidate| candidate.alias_keys),
            Some(BTreeSet::from(["om".into()]))
        );
        assert_eq!(
            normalized_full_path(r"C:\any\home\work\mutil-om"),
            Some("C:/any/home/work/mutil-om".into())
        );
        assert_eq!(
            path_candidate("-c--any-home-work-mutil-om").and_then(|candidate| candidate.full_path),
            None
        );
        assert_eq!(
            path_candidate("-users-lifcc-desktop-code-work-infra-her")
                .map(|candidate| candidate.alias_keys),
            Some(BTreeSet::from(["her".into()]))
        );
    }

    #[test]
    fn path_encoded_hyphenated_and_short_aliases_merge_when_unique() {
        let items = vec![
            observation("slash", "/any/home/work/mutil-om"),
            observation("encoded", "-any-home-work-mutil-om"),
            observation("hyphenated", "mutil-om"),
            observation("short", "om"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.projects.len(), 1);
        assert_eq!(cluster.projects["om"].session_count, 4);
        assert!(cluster
            .item_projects
            .values()
            .all(|project| project == "om"));
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 0);
    }

    #[test]
    fn colliding_absolute_paths_stay_qualified_and_short_alias_is_ambiguous() {
        let items = vec![
            observation("path-a", "/root/a/work/mutil-om"),
            observation("path-b", "/root/b/work/mutil-om"),
            observation("alias", "om"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(
            cluster.item_projects["path-a"],
            "path:/root/a/work/mutil-om"
        );
        assert_eq!(
            cluster.item_projects["path-b"],
            "path:/root/b/work/mutil-om"
        );
        assert_eq!(cluster.item_projects["alias"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 1);
    }

    #[test]
    fn ambiguous_alias_does_not_inherit_a_canonical_project_from_its_document() {
        let mut path = observation("path", "/root/a/work/mutil-om");
        let mut other_path = observation("other-path", "/root/b/work/mutil-om");
        let mut alias = observation("alias", "om");
        path.set_document_id(DocumentId::from("shared"));
        alias.set_document_id(DocumentId::from("shared"));
        other_path.set_document_id(DocumentId::from("other"));
        let items = vec![path, other_path, alias];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.item_projects["path"], "path:/root/a/work/mutil-om");
        assert_eq!(cluster.item_projects["alias"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
    }

    #[test]
    fn combined_window_collision_does_not_drift_between_windows() {
        let current = vec![
            observation("current-path", "/root/a/work/mutil-om"),
            observation("current-alias", "om"),
        ];
        let previous = vec![observation("previous-path", "/root/b/work/mutil-om")];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&current, &previous]);
        let current_cluster = cluster_observations_with_resolver(&current, &resolver);
        let previous_cluster = cluster_observations_with_resolver(&previous, &resolver);

        assert_eq!(
            current_cluster.item_projects["current-path"],
            "path:/root/a/work/mutil-om"
        );
        assert_eq!(current_cluster.item_projects["current-alias"], "other");
        assert_eq!(
            previous_cluster.item_projects["previous-path"],
            "path:/root/b/work/mutil-om"
        );
    }

    #[test]
    fn punctuation_and_path_separators_remain_distinct_full_path_evidence() {
        let items = vec![
            observation("dot", "/r/a.b/foo"),
            observation("hyphen", "/r/a-b/foo"),
            observation("alias", "foo"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.item_projects["dot"], "path:/r/a.b/foo");
        assert_eq!(cluster.item_projects["hyphen"], "path:/r/a-b/foo");
        assert_eq!(cluster.item_projects["alias"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 1);
    }

    #[test]
    fn legacy_inferred_alias_resolves_to_the_explicit_leaf_when_uncontested() {
        let items = vec![
            observation("path", "/users/me/work/my-tools-app"),
            observation("encoded", "-users-me-work-my-tools-app"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.item_projects["path"], "my-tools-app");
        assert_eq!(cluster.item_projects["encoded"], "my-tools-app");
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 0);
    }

    #[test]
    fn independent_bare_project_makes_inferred_alias_fail_closed() {
        let items = vec![
            observation("path", "/users/me/work/my-tools-app"),
            observation("encoded", "-users-me-work-my-tools-app"),
            observation("leaf", "my-tools-app"),
            observation("bare", "app"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.item_projects["path"], "my-tools-app");
        assert_eq!(cluster.item_projects["leaf"], "my-tools-app");
        assert_eq!(cluster.item_projects["encoded"], "other");
        assert_eq!(cluster.item_projects["bare"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 2);
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 1);
    }

    #[test]
    fn hyphenated_repository_name_remains_whole() {
        let items = vec![
            observation("path", "/any/home/ai/tools/page-lingo"),
            observation("alias", "page-lingo"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(
            cluster.global_stats.project_ranking,
            vec![("page-lingo".into(), 2)]
        );
    }
}
