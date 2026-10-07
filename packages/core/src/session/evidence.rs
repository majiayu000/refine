//! Message references validate provenance, not the truth of an extracted claim.

use super::{FacetResponse, SourceMessageReference};
use crate::knowledge::Item;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacetEvidence {
    pub field: String,
    pub index: usize,
    pub message_ids: Vec<i64>,
}

fn fields(facets: &FacetResponse) -> [(&str, usize); 13] {
    [
        ("session_summary", 1),
        ("cognitive_level", 1),
        ("collaboration_mode", 1),
        ("decisions", facets.decisions.len()),
        ("bugs_fixed", facets.bugs_fixed.len()),
        ("patterns", facets.patterns.len()),
        ("friction", facets.friction.len()),
        ("project_progress", facets.project_progress.len()),
        ("questions", facets.questions.len()),
        ("knowledge_gained", facets.knowledge_gained.len()),
        ("tools_discovered", facets.tools_discovered.len()),
        ("architecture", facets.architecture.len()),
        ("code_artifacts", facets.code_artifacts.len()),
    ]
}

pub(super) fn validate_evidence_shape(facets: &FacetResponse) -> Result<(), String> {
    let fields: HashMap<_, _> = fields(facets).into_iter().collect();
    let mut seen = HashSet::new();
    for evidence in &facets.evidence {
        if !fields
            .get(evidence.field.as_str())
            .is_some_and(|count| evidence.index < *count)
        {
            return Err(format!(
                "facet evidence refers to an unknown field/index: {}[{}]",
                evidence.field, evidence.index
            ));
        }
        if !seen.insert((evidence.field.as_str(), evidence.index)) {
            return Err("facet evidence contains a duplicate field/index".into());
        }
        if evidence.message_ids.is_empty() || evidence.message_ids.len() > 32 {
            return Err("facet evidence must contain between 1 and 32 message IDs".into());
        }
        let unique: HashSet<_> = evidence.message_ids.iter().collect();
        if unique.len() != evidence.message_ids.len() {
            return Err("facet evidence contains duplicate message IDs".into());
        }
    }
    Ok(())
}

/// A valid reference must occur in the verified Remem snapshot. Roles/times are
/// never accepted from model output; callers retain the trusted source ledger.
pub fn validate_facet_evidence(
    facets: &FacetResponse,
    source_messages: &[SourceMessageReference],
) -> Result<(), String> {
    validate_evidence_shape(facets)?;
    let ids: HashSet<_> = source_messages.iter().map(|message| message.id).collect();
    if ids.len() != source_messages.len() {
        return Err("source message references contain duplicate IDs".into());
    }
    for evidence in &facets.evidence {
        for id in &evidence.message_ids {
            if !ids.contains(id) {
                return Err(format!(
                    "facet evidence refers to unknown source message ID {id}"
                ));
            }
        }
    }
    Ok(())
}

/// Metadata is scoped to machine candidates. A later human override does not
/// inherit a claim that its changed wording was validated by the extractor.
pub fn session_projection_evidence(
    facets: &FacetResponse,
    source_messages: &[SourceMessageReference],
    machine_items: &[Item],
) -> Result<Value, String> {
    validate_facet_evidence(facets, source_messages)?;
    let supplied: HashMap<_, _> = facets
        .evidence
        .iter()
        .map(|evidence| ((evidence.field.as_str(), evidence.index), evidence))
        .collect();
    let mut entries = Vec::new();
    for (field, count) in fields(facets) {
        for index in 0..count {
            let reference = supplied.get(&(field, index));
            entries.push(json!({
                "field": field,
                "index": index,
                "status": if reference.is_some() { "references_validated" } else { "unknown" },
                "message_ids": reference.map(|evidence| evidence.message_ids.as_slice()).unwrap_or_default(),
            }));
        }
    }
    let mut observations = Vec::new();
    if let Some(summary) = machine_items.first() {
        observations.push(json!({"item_id": summary.id(), "facets": "all"}));
    }
    for (index, item) in machine_items
        .iter()
        .skip(1)
        .take(facets.decisions.len())
        .enumerate()
    {
        observations.push(json!({"item_id": item.id(), "field": "decisions", "index": index}));
    }
    for (index, item) in machine_items
        .iter()
        .skip(1 + facets.decisions.len())
        .take(facets.bugs_fixed.len())
        .enumerate()
    {
        observations.push(json!({"item_id": item.id(), "field": "bugs_fixed", "index": index}));
    }
    Ok(json!({
        "schema_version": 1,
        "scope": "machine_candidates",
        "source_messages": source_messages,
        "facets": entries,
        "observations": observations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{parse_facet_response, MessageRole};
    use chrono::{DateTime, Utc};

    fn facets(evidence: Value) -> FacetResponse {
        parse_facet_response(
            &json!({
                "session_summary": "讨论缓存方案",
                "cognitive_level": "unknown",
                "collaboration_mode": "exploration",
                "decisions": ["用户确认保留缓存"],
                "evidence": evidence,
            })
            .to_string(),
        )
        .unwrap()
    }

    fn sources() -> Vec<SourceMessageReference> {
        vec![
            SourceMessageReference {
                id: 41,
                role: MessageRole::Assistant,
                event_time: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            },
            SourceMessageReference {
                id: 42,
                role: MessageRole::User,
                event_time: DateTime::<Utc>::from_timestamp(1_700_000_100, 0).unwrap(),
            },
        ]
    }

    #[test]
    fn absent_evidence_is_unknown_even_when_raw_messages_exist() {
        let metadata = session_projection_evidence(&facets(json!([])), &sources(), &[]).unwrap();
        assert!(metadata["facets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["status"] == "unknown"));
        assert_eq!(metadata["source_messages"][0]["role"], "assistant");
        assert_eq!(metadata["source_messages"][1]["role"], "user");
        assert_eq!(
            metadata["source_messages"][1]["event_time"],
            "2023-11-14T22:15:00Z"
        );
    }

    #[test]
    fn unknown_ids_and_model_supplied_roles_are_rejected() {
        let invalid = facets(json!([{"field": "decisions", "index": 0, "message_ids": [999]}]));
        assert!(validate_facet_evidence(&invalid, &sources())
            .unwrap_err()
            .contains("999"));
        let valid = facets(json!([{"field": "decisions", "index": 0, "message_ids": [41, 42]}]));
        validate_facet_evidence(&valid, &sources()).unwrap();
        assert!(validate_facet_evidence(&valid, &[]).is_err());
        let malformed = json!({"session_summary": "test", "cognitive_level": "unknown", "collaboration_mode": "unknown", "evidence": [{"field": "session_summary", "index": 0, "message_ids": [41], "role": "user"}]});
        assert!(parse_facet_response(&malformed.to_string()).is_err());
    }

    #[test]
    fn evidence_shape_rejects_dangling_and_duplicate_references() {
        for evidence in [
            json!([{"field": "decisions", "index": 1, "message_ids": [41]}]),
            json!([{"field": "error", "index": 0, "message_ids": [41]}]),
            json!([{"field": "session_summary", "index": 0, "message_ids": []}]),
            json!([{"field": "session_summary", "index": 0, "message_ids": [41, 41]}]),
            json!([{"field": "session_summary", "index": 0, "message_ids": [41]}, {"field": "session_summary", "index": 0, "message_ids": [42]}]),
        ] {
            let value = json!({"session_summary": "test", "cognitive_level": "unknown", "collaboration_mode": "unknown", "evidence": evidence});
            assert!(parse_facet_response(&value.to_string()).is_err());
        }
    }
}
