use chrono::{DateTime, Utc};
use refine_core::knowledge::Item;
use std::collections::BTreeMap;

use super::super::bundle::{
    DimensionEvidence, DimensionProjection, PortraitDimensions, MAX_DIMENSION_ENTRIES,
    MAX_DIMENSION_EVIDENCE_IDS,
};
use super::hashing::{sha256_bytes, truncate_projection_text, MultisetDigest, StableDigest};

#[derive(Clone, Copy)]
enum Kind {
    Projects,
    Decisions,
    Bugfixes,
    Knowledge,
    Patterns,
    Architectures,
    Frictions,
}

#[derive(Default)]
struct Sample {
    value: String,
    original_bytes: usize,
    value_digest: String,
    support_count: usize,
    evidence: Vec<(DateTime<Utc>, String)>,
}

#[derive(Default)]
struct Accumulator {
    total_occurrences: usize,
    full_digest: MultisetDigest,
    samples: BTreeMap<String, Sample>,
}

impl Accumulator {
    fn sample(&mut self, value: &str) {
        let value_digest = sha256_bytes(value.as_bytes());
        if self.samples.contains_key(&value_digest) {
            return;
        }
        if self.samples.len() == MAX_DIMENSION_ENTRIES {
            let largest = self.samples.last_key_value().map(|(key, _)| key.clone());
            if largest.as_ref().is_some_and(|key| value_digest >= *key) {
                return;
            }
            if let Some(largest) = largest {
                self.samples.remove(&largest);
            }
        }
        self.samples.insert(
            value_digest.clone(),
            Sample {
                value: truncate_projection_text(value),
                original_bytes: value.len(),
                value_digest,
                ..Sample::default()
            },
        );
    }

    fn observe(&mut self, value: &str, item_id: &str, event_time: DateTime<Utc>, selected: bool) {
        self.total_occurrences += 1;
        let mut row = StableDigest::new("cognitive-portrait-dimension-row-v2");
        row.text(value);
        row.text(item_id);
        self.full_digest.add(row.finish_bytes());
        let digest = sha256_bytes(value.as_bytes());
        if let Some(sample) = self.samples.get_mut(&digest) {
            sample.support_count += 1;
            if selected
                && !sample
                    .evidence
                    .iter()
                    .any(|(_, evidence_id)| evidence_id == item_id)
            {
                sample.evidence.push((event_time, item_id.to_string()));
                sample
                    .evidence
                    .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
                sample.evidence.truncate(MAX_DIMENSION_EVIDENCE_IDS);
            }
        }
    }

    fn finish(self) -> DimensionProjection {
        let total_occurrences = self.total_occurrences;
        let full_digest = self
            .full_digest
            .finish("cognitive-portrait-dimension-multiset-v2");
        let mut samples: Vec<Sample> = self
            .samples
            .into_values()
            .filter(|sample| sample.support_count > 0 && !sample.evidence.is_empty())
            .collect();
        samples.sort_by(|left, right| {
            right
                .support_count
                .cmp(&left.support_count)
                .then_with(|| right.evidence[0].0.cmp(&left.evidence[0].0))
                .then_with(|| left.value_digest.cmp(&right.value_digest))
        });
        let entries: Vec<DimensionEvidence> = samples
            .into_iter()
            .map(|sample| {
                let evidence_ids: Vec<String> = sample
                    .evidence
                    .into_iter()
                    .map(|(_, item_id)| format!("obs:{item_id}"))
                    .collect();
                DimensionEvidence {
                    value: sample.value,
                    original_bytes: sample.original_bytes,
                    value_digest: sample.value_digest,
                    support_count: sample.support_count,
                    omitted_evidence_count: sample.support_count - evidence_ids.len(),
                    evidence_ids,
                }
            })
            .collect();
        let selected_occurrences = entries.iter().map(|entry| entry.support_count).sum();
        let selected_evidence_refs = entries.iter().map(|entry| entry.evidence_ids.len()).sum();
        DimensionProjection {
            total_occurrences,
            selected_occurrences,
            omitted_occurrences: total_occurrences - selected_occurrences,
            selected_values: entries.len(),
            selected_evidence_refs,
            full_digest,
            entries,
        }
    }
}

#[derive(Default)]
pub(super) struct DimensionAccumulators {
    projects: Accumulator,
    decisions: Accumulator,
    bugfixes: Accumulator,
    knowledge: Accumulator,
    patterns: Accumulator,
    architectures: Accumulator,
    frictions: Accumulator,
}

impl DimensionAccumulators {
    pub(super) fn sample_item(&mut self, item: &Item, project: &str) {
        for_each_value(item, project, |kind, value| {
            self.get_mut(kind).sample(value)
        });
    }

    pub(super) fn observe_item(
        &mut self,
        item: &Item,
        project: &str,
        event_time: DateTime<Utc>,
        selected: bool,
    ) {
        for_each_value(item, project, |kind, value| {
            self.get_mut(kind)
                .observe(value, item.id().as_str(), event_time, selected)
        });
    }

    pub(super) fn finish(self) -> PortraitDimensions {
        PortraitDimensions {
            projects: self.projects.finish(),
            decisions: self.decisions.finish(),
            bugfixes: self.bugfixes.finish(),
            knowledge: self.knowledge.finish(),
            patterns: self.patterns.finish(),
            architectures: self.architectures.finish(),
            frictions: self.frictions.finish(),
        }
    }

    fn get_mut(&mut self, kind: Kind) -> &mut Accumulator {
        match kind {
            Kind::Projects => &mut self.projects,
            Kind::Decisions => &mut self.decisions,
            Kind::Bugfixes => &mut self.bugfixes,
            Kind::Knowledge => &mut self.knowledge,
            Kind::Patterns => &mut self.patterns,
            Kind::Architectures => &mut self.architectures,
            Kind::Frictions => &mut self.frictions,
        }
    }
}

fn for_each_value(item: &Item, project: &str, mut visit: impl FnMut(Kind, &str)) {
    visit(Kind::Projects, project);
    let tags: Vec<&str> = item.tags().iter().map(|tag| tag.as_str()).collect();
    if tags.contains(&"decision") {
        visit(Kind::Decisions, item.title());
    } else if tags.contains(&"bugfix") {
        visit(Kind::Bugfixes, item.title());
    }
    for (kind, section) in [
        (Kind::Knowledge, "知识"),
        (Kind::Patterns, "模式"),
        (Kind::Architectures, "架构"),
        (Kind::Frictions, "阻力"),
    ] {
        super::for_each_section_item(item.content(), section, |value| visit(kind, value));
    }
}
