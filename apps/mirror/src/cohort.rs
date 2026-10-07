//! All Mirror outputs use the same source-aware cohort contract as Insights.
//! One repository snapshot supplies every compared window and its source metadata.
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::knowledge::{ItemRepository, ObservationWindowSnapshot};
use refine_core::session::{cluster_session_observation_windows, SessionCohortCluster};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub(crate) struct EventWindow {
    pub start: Option<DateTime<Utc>>,
    pub end: DateTime<Utc>,
}

impl EventWindow {
    pub fn from_options(since: Option<&str>, all: bool, cutoff: DateTime<Utc>) -> Result<Self> {
        if all && since.is_some() {
            anyhow::bail!("--all and --since are mutually exclusive");
        }
        if all {
            return Ok(Self {
                start: None,
                end: cutoff,
            });
        }
        let Some(since) = since else {
            return Ok(Self::rolling(90, cutoff));
        };
        let start = chrono::NaiveDate::parse_from_str(since, "%Y-%m-%d")
            .with_context(|| format!("invalid --since date '{since}'"))?
            .and_hms_opt(0, 0, 0)
            .context("invalid date")?
            .and_utc();
        Ok(Self {
            start: Some(start),
            end: cutoff,
        })
    }

    pub fn rolling(days: i64, cutoff: DateTime<Utc>) -> Self {
        Self {
            start: Some(cutoff - Duration::days(days)),
            end: cutoff,
        }
    }
}

pub(crate) async fn load_cohorts(
    repository: &dyn ItemRepository,
    cutoff: DateTime<Utc>,
    windows: &[EventWindow],
) -> Result<Vec<SessionCohortCluster>> {
    let earliest = windows.iter().filter_map(|window| window.start).min();
    let all_history = windows.iter().any(|window| window.start.is_none());
    let period = if all_history {
        None
    } else {
        let days = earliest
            .map(|start| (cutoff - start).num_days().max(0) as usize + 1)
            .unwrap_or(90);
        // The existing two-window snapshot API covers 2 * period days. Merge
        // those rows before selecting our exact windows; no extra DB reads.
        Some(days.div_ceil(2).max(1))
    };
    let snapshot = repository
        .load_observation_window_snapshot(cutoff, period)
        .await
        .context("load Mirror observations and source metadata from one snapshot")?;
    Ok(cluster_windows(snapshot, windows))
}

fn cluster_windows(
    snapshot: ObservationWindowSnapshot,
    windows: &[EventWindow],
) -> Vec<SessionCohortCluster> {
    let sources = snapshot
        .documents
        .iter()
        .map(|document| (document.id.as_str().to_string(), document.source.clone()))
        .collect::<HashMap<_, _>>();
    let event_times = snapshot
        .documents
        .iter()
        .map(|document| (document.id.as_str(), document.captured_at))
        .collect::<HashMap<_, _>>();
    let items = snapshot
        .current
        .into_iter()
        .chain(snapshot.previous)
        .collect::<Vec<_>>();
    let selected = windows
        .iter()
        .map(|window| {
            items
                .iter()
                .filter(|item| {
                    let event_time = item
                        .document_id()
                        .and_then(|id| event_times.get(id.as_str()))
                        .copied()
                        .unwrap_or_else(|| item.created_at());
                    window.start.is_none_or(|start| event_time >= start) && event_time < window.end
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let slices = selected.iter().map(Vec::as_slice).collect::<Vec<_>>();
    cluster_session_observation_windows(&slices, &sources)
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::knowledge::{DocumentId, Item, ObservationDocumentMeta, Tag};

    #[test]
    fn mirror_windows_share_source_exclusion_and_document_event_times() {
        let now = Utc::now();
        let sources = [
            ("current", "codex-session", 1),
            ("previous", "remem-raw-session", 9),
            ("knowledge", "grok-knowledge", 1),
        ];
        let mut current = Vec::new();
        let mut documents = Vec::new();
        for (id, source, days_ago) in sources {
            let mut item = Item::new_observation(id, "synthetic evidence");
            item.set_document_id(DocumentId::from(id));
            item.set_tags(vec![
                Tag::new("project").unwrap(),
                Tag::new("expert").unwrap(),
            ])
            .unwrap();
            current.push(item);
            documents.push(ObservationDocumentMeta {
                id: DocumentId::from(id),
                source: source.into(),
                captured_at: now - Duration::days(days_ago),
            });
        }
        let windows = [
            EventWindow::rolling(90, now),
            EventWindow::rolling(7, now),
            EventWindow {
                start: Some(now - Duration::days(14)),
                end: now - Duration::days(7),
            },
        ];
        let cohorts = cluster_windows(
            ObservationWindowSnapshot {
                current,
                previous: Vec::new(),
                documents,
            },
            &windows,
        );
        assert_eq!(cohorts[0].cluster.global_stats.total_sessions, 2);
        assert_eq!(cohorts[1].cluster.global_stats.total_sessions, 1);
        assert_eq!(cohorts[2].cluster.global_stats.total_sessions, 1);
        assert_eq!(
            cohorts[0].cluster.data_quality.source_excluded_observations,
            1
        );
        assert_eq!(
            cohorts[1].cluster.data_quality.source_excluded_observations,
            1
        );
        assert_eq!(
            cohorts[2].cluster.data_quality.source_excluded_observations,
            0
        );
        assert!(cohorts[2]
            .cohort_items
            .iter()
            .all(|item| item.document_id().unwrap().as_str() == "previous"));
    }
}
