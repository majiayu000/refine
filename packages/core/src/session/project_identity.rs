use super::clustering::{
    eligible_observations, is_generic_project_path_segment, is_project_meta_tag, is_session_id,
};
use super::facets::SESSION_PROJECT_SOURCE_PLATFORM;
#[cfg(test)]
use super::project_identity_value::normalized_path_display;
use super::project_identity_value::{
    bounded_hyphen_join, encoded_path_key, encoded_path_value, raw_path_key, raw_path_value,
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
    key: String,
    qualified_identity: String,
    encoded_key: String,
    display_alias: Option<String>,
    display_is_explicit: bool,
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
    path_resolutions: HashMap<String, ProjectResolution>,
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
        let mut candidates = BTreeMap::new();
        let mut bare_aliases = BTreeSet::new();
        for item in items {
            if let Some(project) = structured_project(item) {
                record_candidate(project, &mut candidates, &mut bare_aliases);
            } else {
                for tag in item.tags() {
                    let raw = tag.as_str();
                    if !is_project_meta_tag(raw) {
                        record_candidate(raw, &mut candidates, &mut bare_aliases);
                    }
                }
            }
        }

        let raw_by_encoded_key: BTreeMap<String, BTreeSet<String>> = candidates
            .values()
            .filter(|candidate| candidate.display_is_explicit)
            .fold(BTreeMap::new(), |mut by_encoded, candidate| {
                by_encoded
                    .entry(candidate.encoded_key.clone())
                    .or_default()
                    .insert(candidate.key.clone());
                by_encoded
            });
        let encoded_keys: Vec<String> = candidates
            .values()
            .filter(|candidate| !candidate.display_is_explicit)
            .map(|candidate| candidate.key.clone())
            .collect();
        let mut matched_encoded = BTreeMap::new();
        let mut ambiguous_encoded = BTreeMap::new();
        for encoded_key in encoded_keys {
            let Some(candidate) = candidates.get(&encoded_key).cloned() else {
                continue;
            };
            match raw_by_encoded_key.get(&candidate.encoded_key) {
                Some(raw_keys) if raw_keys.len() == 1 => {
                    if let Some(raw_key) = raw_keys.iter().next() {
                        matched_encoded.insert(encoded_key.clone(), raw_key.clone());
                        candidates.remove(&encoded_key);
                    }
                }
                Some(raw_keys) if raw_keys.len() > 1 => {
                    let alias = candidate
                        .alias_keys
                        .iter()
                        .next()
                        .cloned()
                        .unwrap_or_else(|| candidate.encoded_key.clone());
                    ambiguous_encoded.insert(encoded_key.clone(), alias);
                    candidates.remove(&encoded_key);
                }
                _ => {}
            }
        }

        let mut alias_candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for candidate in candidates.values() {
            for alias in &candidate.alias_keys {
                alias_candidates
                    .entry(alias.clone())
                    .or_default()
                    .insert(candidate.key.clone());
            }
        }

        let canonical_candidates: HashMap<String, String> = candidates
            .values()
            .map(|candidate| {
                let has_collision = candidate.alias_keys.iter().any(|alias| {
                    alias_candidates
                        .get(alias)
                        .is_some_and(|candidate_keys| candidate_keys.len() > 1)
                });
                let inferred_alias_contested = !candidate.display_is_explicit
                    && candidate
                        .alias_keys
                        .iter()
                        .any(|alias| bare_aliases.contains(alias));
                let identity_is_bounded = candidate.qualified_identity.contains("~%bytes=");
                let canonical = if has_collision || inferred_alias_contested || identity_is_bounded
                {
                    candidate.qualified_identity.clone()
                } else {
                    candidate
                        .display_alias
                        .clone()
                        .unwrap_or_else(|| candidate.qualified_identity.clone())
                };
                (candidate.key.clone(), canonical)
            })
            .collect();

        let aliases = alias_candidates
            .into_iter()
            .map(|(alias, candidate_keys)| {
                let resolution = if candidate_keys.len() != 1 {
                    AliasResolution::Ambiguous
                } else {
                    let candidate_key = candidate_keys.iter().next();
                    let is_explicit_display_alias = candidate_key
                        .and_then(|candidate_key| candidates.get(candidate_key))
                        .is_some_and(|candidate| {
                            candidate.display_is_explicit
                                && candidate.display_alias.as_deref() == Some(alias.as_str())
                        });
                    if bare_aliases.contains(&alias) && !is_explicit_display_alias {
                        AliasResolution::Ambiguous
                    } else {
                        candidate_key
                            .and_then(|candidate_key| canonical_candidates.get(candidate_key))
                            .cloned()
                            .map(AliasResolution::Canonical)
                            .unwrap_or(AliasResolution::Ambiguous)
                    }
                };
                (alias, resolution)
            })
            .collect();

        let mut path_resolutions: HashMap<String, ProjectResolution> = canonical_candidates
            .into_iter()
            .map(|(key, canonical)| (key, ProjectResolution::Canonical(canonical)))
            .collect();
        for (encoded_key, raw_key) in matched_encoded {
            if let Some(resolution) = path_resolutions.get(&raw_key).cloned() {
                path_resolutions.insert(encoded_key, resolution);
            }
        }
        for (encoded_key, alias) in ambiguous_encoded {
            path_resolutions.insert(encoded_key, ProjectResolution::AmbiguousAlias(alias));
        }

        Self {
            path_resolutions,
            aliases,
        }
    }

    pub(super) fn resolve_item(&self, item: &Item, tags: &[&str]) -> Option<ProjectResolution> {
        if let Some(project) = structured_project(item) {
            return self.resolve_inputs(std::iter::once(project));
        }
        self.resolve_inputs(tags.iter().copied())
    }

    fn resolve_inputs<'a>(
        &self,
        inputs: impl IntoIterator<Item = &'a str>,
    ) -> Option<ProjectResolution> {
        let mut canonical_paths = Vec::new();
        let mut canonical_aliases = Vec::new();
        let mut ambiguous_aliases = Vec::new();

        for raw in inputs.into_iter().filter(|tag| !is_project_meta_tag(tag)) {
            if let Some(key) = path_candidate_key(raw) {
                match self.path_resolutions.get(&key) {
                    Some(ProjectResolution::Canonical(canonical)) => {
                        canonical_paths.push(canonical.clone());
                    }
                    Some(ProjectResolution::AmbiguousAlias(alias)) => {
                        ambiguous_aliases.push(alias.clone());
                    }
                    None => {
                        if let Some(path) = path_candidate(raw) {
                            canonical_paths.push(path.qualified_identity);
                        }
                    }
                }
                continue;
            }
            let Some(alias) = normalize_project_name_bounded(raw) else {
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

fn structured_project(item: &Item) -> Option<&str> {
    item.source()
        .filter(|source| source.platform == SESSION_PROJECT_SOURCE_PLATFORM)
        .and_then(|source| source.url.as_deref())
        .map(str::trim)
        .filter(|project| !project.is_empty())
}

fn record_candidate(
    raw: &str,
    candidates: &mut BTreeMap<String, PathCandidate>,
    bare_aliases: &mut BTreeSet<String>,
) {
    if let Some(key) = path_candidate_key(raw) {
        if candidates.contains_key(&key) {
            return;
        }
    }
    if let Some(candidate) = path_candidate(raw) {
        candidates
            .entry(candidate.key.clone())
            .and_modify(|existing| {
                existing.alias_keys.extend(candidate.alias_keys.clone());
            })
            .or_insert(candidate);
    } else if let Some(alias) = normalize_project_name_bounded(raw) {
        bare_aliases.insert(alias);
    }
}

fn path_candidate_key(raw: &str) -> Option<String> {
    if is_encoded_path(raw) {
        encoded_path_key(raw, is_session_id)
    } else if looks_like_path(raw) {
        raw_path_key(raw, is_session_id)
    } else {
        None
    }
}

fn path_candidate(raw: &str) -> Option<PathCandidate> {
    let mut alias_keys = BTreeSet::new();
    if is_encoded_path(raw) {
        let value = encoded_path_value(raw, is_session_id)?;
        let display_alias = normalize_encoded_path_alias(raw);
        if let Some(alias) = &display_alias {
            alias_keys.insert(alias.clone());
        }
        return Some(PathCandidate {
            key: value.key,
            qualified_identity: value.qualified,
            encoded_key: value.encoded_fingerprint,
            display_alias,
            display_is_explicit: false,
            alias_keys,
        });
    }

    if !looks_like_path(raw) {
        return None;
    }
    let value = raw_path_value(raw, is_session_id)?;
    let display_alias = path_leaf(raw).and_then(normalize_project_name_bounded);
    if let Some(alias) = &display_alias {
        alias_keys.insert(alias.clone());
    }
    if let Some(encoded_alias) = normalize_encoded_path_alias(raw) {
        alias_keys.insert(encoded_alias);
    }
    Some(PathCandidate {
        key: value.key,
        qualified_identity: value.qualified,
        encoded_key: value.encoded_fingerprint,
        display_alias,
        display_is_explicit: true,
        alias_keys,
    })
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
    let segment_count = legacy_segments(raw).count();
    let last_generic = legacy_segments(raw)
        .enumerate()
        .filter(|(index, _)| *index + 1 < segment_count)
        .filter(|(_, segment)| is_legacy_generic_segment(segment))
        .map(|(index, _)| index)
        .last();
    let start = last_generic.map_or(0, |index| index + 1);
    normalize_project_segments_bounded(legacy_segments(raw).skip(start))
}

fn legacy_segments(raw: &str) -> impl DoubleEndedIterator<Item = &str> {
    raw.split(['-', '/', '\\', '.', ':'])
        .filter(|segment| !segment.is_empty())
}

fn normalize_project_name_bounded(raw: &str) -> Option<String> {
    normalize_project_segments_bounded(raw.split('-').filter(|segment| !segment.is_empty()))
}

fn normalize_project_segments_bounded<'a>(
    segments: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut found_project = false;
    bounded_hyphen_join(segments.into_iter().filter(|segment| {
        if !found_project {
            if is_generic_project_path_segment(segment) {
                return false;
            }
            found_project = true;
        }
        !is_session_id(segment)
    }))
}

fn is_legacy_generic_segment(segment: &str) -> bool {
    is_generic_project_path_segment(segment) || matches!(segment, "infra" | "gateway")
}

#[cfg(test)]
mod tests {
    use super::super::clustering::cluster_observations_with_resolver;
    use super::*;
    use crate::knowledge::{DocumentId, ItemId, ItemType, RestoreParams, Source, Tag};
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

    fn observation_with_structured_project(id: &str, project: &str) -> Item {
        let mut item = observation(id, project);
        item.set_source(Source::new(SESSION_PROJECT_SOURCE_PLATFORM).with_url(project));
        item
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
            normalized_path_display(r"C:\any\home\work\mutil-om", is_session_id),
            Some("C:/any/home/work/mutil-om".into())
        );
        assert_eq!(
            path_candidate("-c--any-home-work-mutil-om")
                .map(|candidate| candidate.qualified_identity),
            Some("encoded:-c--any-home-work-mutil-om".into())
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
        assert_eq!(cluster.item_projects["encoded"], "my-tools-app");
        assert_eq!(cluster.item_projects["bare"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 1);
    }

    #[test]
    fn distinct_encoded_cwds_collide_without_losing_direct_identity() {
        let items = vec![
            observation("encoded-a", "-root-a-work-mutil-om"),
            observation("encoded-b", "-root-b-work-mutil-om"),
            observation("alias", "om"),
        ];
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(
            cluster.item_projects["encoded-a"],
            "encoded:-root-a-work-mutil-om"
        );
        assert_eq!(
            cluster.item_projects["encoded-b"],
            "encoded:-root-b-work-mutil-om"
        );
        assert_eq!(cluster.item_projects["alias"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
        assert_eq!(cluster.data_quality.ambiguous_project_aliases, 1);
    }

    #[test]
    fn structured_project_identity_preserves_unix_path_case_through_tags() {
        let items = vec![
            observation_with_structured_project("upper", "/r/Foo/bar"),
            observation_with_structured_project("lower", "/r/foo/bar"),
            observation("alias", "bar"),
        ];
        assert_eq!(items[0].tags()[0].as_str(), "/r/foo/bar");
        let resolver = ProjectIdentityResolver::from_observation_windows(&[&items]);
        let cluster = cluster_observations_with_resolver(&items, &resolver);

        assert_eq!(cluster.item_projects["upper"], "path:/r/Foo/bar");
        assert_eq!(cluster.item_projects["lower"], "path:/r/foo/bar");
        assert_eq!(cluster.item_projects["alias"], "other");
        assert_eq!(cluster.data_quality.ambiguous_project_alias_observations, 1);
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
