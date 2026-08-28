use chrono::{DateTime, Utc};
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BinaryHeap};

use super::super::bundle::{SelectionStratum, MAX_SELECTED_EVIDENCE_PER_WINDOW};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct StratumKey {
    pub(super) source: String,
    pub(super) category: String,
    pub(super) project_bucket: String,
}

#[derive(Debug, Clone, Eq)]
struct SelectedIndex {
    index: usize,
    event_time: DateTime<Utc>,
    item_id: String,
}

impl PartialEq for SelectedIndex {
    fn eq(&self, other: &Self) -> bool {
        self.event_time == other.event_time && self.item_id == other.item_id
    }
}

impl Ord for SelectedIndex {
    fn cmp(&self, other: &Self) -> Ordering {
        self.event_time
            .cmp(&other.event_time)
            .then_with(|| other.item_id.cmp(&self.item_id))
    }
}

impl PartialOrd for SelectedIndex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) fn allocate_quotas(counts: &BTreeMap<StratumKey, usize>) -> BTreeMap<StratumKey, usize> {
    let limit = counts
        .values()
        .sum::<usize>()
        .min(MAX_SELECTED_EVIDENCE_PER_WINDOW);
    let mut quotas: BTreeMap<StratumKey, usize> =
        counts.keys().cloned().map(|key| (key, 0)).collect();
    let mut allocated = 0usize;
    while allocated < limit {
        let mut progressed = false;
        for (key, count) in counts {
            if allocated == limit {
                break;
            }
            let quota = quotas.get_mut(key).expect("quota key comes from counts");
            if *quota < *count {
                *quota += 1;
                allocated += 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    quotas
}

pub(super) struct BoundedSelection {
    quotas: BTreeMap<StratumKey, usize>,
    selected: BTreeMap<StratumKey, BinaryHeap<Reverse<SelectedIndex>>>,
}

impl BoundedSelection {
    pub(super) fn new(quotas: BTreeMap<StratumKey, usize>) -> Self {
        Self {
            quotas,
            selected: BTreeMap::new(),
        }
    }

    pub(super) fn consider(
        &mut self,
        key: StratumKey,
        index: usize,
        event_time: DateTime<Utc>,
        item_id: &str,
    ) {
        let quota = self.quotas.get(&key).copied().unwrap_or(0);
        if quota == 0 {
            return;
        }
        let heap = self.selected.entry(key).or_default();
        let candidate = SelectedIndex {
            index,
            event_time,
            item_id: item_id.to_string(),
        };
        if heap.len() < quota {
            heap.push(Reverse(candidate));
        } else if heap.peek().is_some_and(|Reverse(worst)| candidate > *worst) {
            heap.pop();
            heap.push(Reverse(candidate));
        }
    }

    pub(super) fn into_ranked(self) -> Vec<(StratumKey, Vec<usize>)> {
        self.selected
            .into_iter()
            .map(|(key, heap)| {
                let mut selected: Vec<SelectedIndex> =
                    heap.into_iter().map(|Reverse(value)| value).collect();
                selected.sort_by(|left, right| right.cmp(left));
                (key, selected.into_iter().map(|value| value.index).collect())
            })
            .collect()
    }
}

pub(super) fn next_evidence_json_bytes(
    current_bytes: usize,
    selected_count: usize,
    record_bytes: usize,
    byte_budget: usize,
) -> Option<usize> {
    let separator_bytes = usize::from(selected_count > 0);
    current_bytes
        .checked_add(separator_bytes)?
        .checked_add(record_bytes)
        .filter(|bytes| *bytes <= byte_budget)
}

pub(super) fn build_strata(
    counts: BTreeMap<StratumKey, usize>,
    selected_counts: &BTreeMap<StratumKey, usize>,
) -> Vec<SelectionStratum> {
    counts
        .into_iter()
        .map(|(key, eligible_observations)| {
            let selected_observations = selected_counts.get(&key).copied().unwrap_or(0);
            SelectionStratum {
                source: key.source,
                category: key.category,
                project_bucket: key.project_bucket,
                eligible_observations,
                selected_observations,
                omitted_observations: eligible_observations - selected_observations,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn key(name: &str) -> StratumKey {
        StratumKey {
            source: "codex".to_string(),
            category: "summary".to_string(),
            project_bucket: name.to_string(),
        }
    }

    #[test]
    fn json_budget_accepts_exact_boundary_and_rejects_boundary_plus_one() {
        assert_eq!(next_evidence_json_bytes(2, 0, 10, 12), Some(12));
        assert_eq!(next_evidence_json_bytes(2, 0, 10, 11), None);
        let escaped = [
            "认知".repeat(64),
            "😀".repeat(64),
            "\u{1}".repeat(512),
            "\"".repeat(256),
            "\\".repeat(256),
        ];
        let record_bytes: Vec<usize> = escaped
            .iter()
            .map(|value| serde_json::to_vec(value).unwrap().len())
            .collect();
        assert!(record_bytes[2] > escaped[2].len() * 5);
        let exact = 2 + record_bytes.iter().sum::<usize>() + record_bytes.len() - 1;
        let mut current = 2usize;
        for (index, bytes) in record_bytes.iter().enumerate() {
            current = next_evidence_json_bytes(current, index, *bytes, exact).unwrap();
        }
        assert_eq!(current, exact);
        assert_eq!(
            next_evidence_json_bytes(current, record_bytes.len(), 1, exact),
            None
        );
    }

    #[test]
    fn quota_selection_is_stratified_bounded_and_stable() {
        let counts = BTreeMap::from([(key("a"), 2), (key("b"), 1)]);
        let quotas = allocate_quotas(&counts);
        assert_eq!(quotas, counts);
        let mut selector = BoundedSelection::new(quotas);
        let time = Utc.with_ymd_and_hms(2026, 8, 20, 0, 0, 0).unwrap();
        selector.consider(key("a"), 0, time, "z");
        selector.consider(key("a"), 1, time, "a");
        selector.consider(key("b"), 2, time, "b");
        assert_eq!(
            selector.into_ranked(),
            vec![(key("a"), vec![1, 0]), (key("b"), vec![2])]
        );
    }
}
