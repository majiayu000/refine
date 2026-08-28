use super::bundle::{CognitivePortraitBundle, PortraitClaim, PORTRAIT_CLAIM_CATALOG_VERSION};
use super::{MAX_PORTRAIT_CANDIDATE_BYTES, MAX_PREVIOUS_PORTRAIT_BYTES};
use anyhow::{Context, Result};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

mod markdown;
mod novelty;
use markdown::{scan_markdown, VisibleBlock, VisibleBlockKind};
use novelty::novelty_rate;

pub(crate) const PORTRAIT_QUALITY_GATE_VERSION: &str = "cognitive-portrait-quality-v1";
const DEFAULT_NOVELTY_RATE: f64 = 0.60;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PortraitQualityReport {
    pub gate_version: String,
    pub passed: bool,
    pub factual_claims: usize,
    pub traceable_factual_claims: usize,
    pub factual_traceability_rate: f64,
    pub numeric_claims: usize,
    pub unsupported_numeric_claims: usize,
    pub unsupported_number_rate: f64,
    pub inference_claims: usize,
    pub traceable_inference_claims: usize,
    pub inference_traceability_rate: f64,
    pub comparable_cohort_rate: f64,
    pub comparison_claims_suppressed: bool,
    pub action_claims: usize,
    pub verifiable_actions: usize,
    pub action_verifiability_rate: f64,
    pub novelty_rate: Option<f64>,
    pub repetition_rate: Option<f64>,
    pub structure_complete: bool,
    pub errors: Vec<String>,
}

pub(super) fn write_quality_report(path: &Path, report: &PortraitQualityReport) -> Result<()> {
    let mut json = serde_json::to_string_pretty(report).context("serialize portrait quality")?;
    json.push('\n');
    fs::write(path, json)
        .with_context(|| format!("write portrait quality report {}", path.display()))
}

pub(crate) fn validate_portrait(
    bundle: &CognitivePortraitBundle,
    candidate: &str,
    previous_portrait: Option<&str>,
) -> PortraitQualityReport {
    if candidate.len() > MAX_PORTRAIT_CANDIDATE_BYTES {
        return failed_quality_report(format!(
            "portrait candidate exceeds the {MAX_PORTRAIT_CANDIDATE_BYTES} byte limit"
        ));
    }
    let previous_oversized =
        previous_portrait.is_some_and(|previous| previous.len() > MAX_PREVIOUS_PORTRAIT_BYTES);
    let scan = scan_markdown(candidate);
    let blocks = scan.blocks;
    let bundle_json = serde_json::to_value(bundle).expect("portrait bundle must serialize");
    let evidence_ids: HashSet<&str> = bundle
        .current
        .evidence
        .iter()
        .chain(bundle.previous.evidence.iter())
        .map(|evidence| evidence.evidence_id.as_str())
        .collect();
    let catalog: HashMap<&str, &PortraitClaim> = bundle
        .claim_catalog
        .claims
        .iter()
        .map(|claim| (claim.claim_id.as_str(), claim))
        .collect();
    let mut used_claims = HashSet::new();
    let mut factual_claims = 0usize;
    let mut traceable_factual_claims = 0usize;
    let mut numeric_claims = 0usize;
    let mut unsupported_numeric_claims = 0usize;
    let mut inference_claims = 0usize;
    let mut traceable_inference_claims = 0usize;
    let mut action_claims = 0usize;
    let mut verifiable_actions = 0usize;
    let mut errors = scan.violations;
    errors.extend(validate_structure(&blocks));
    if previous_oversized {
        errors.push(format!(
            "previous portrait exceeds the {MAX_PREVIOUS_PORTRAIT_BYTES} byte limit"
        ));
    }

    if bundle.claim_catalog.schema_version != PORTRAIT_CLAIM_CATALOG_VERSION
        || catalog.len() != bundle.claim_catalog.claims.len()
    {
        errors.push("claim catalog schema or claim IDs are invalid".to_string());
    }
    if !bundle.comparison.comparable {
        errors.push(format!(
            "comparison is DEGRADED; portrait generation is disabled: {}",
            bundle.comparison.reasons.join(",")
        ));
    }

    for block in paragraph_blocks(&blocks) {
        let line = block.text.as_str();
        let claim_id = field_value(line, "claim");
        let catalog_claim = claim_id.and_then(|id| catalog.get(id).copied());
        let is_catalog_line = catalog_claim
            .is_some_and(|claim| claim.rendered_line == line && claim.rendered_line == block.raw);
        let has_numeric = contains_numeric_token(line);
        let is_factual = line.contains("[事实]");
        let is_inference = line.contains("[推断");
        let unique_claim = claim_id.is_some_and(|id| used_claims.insert(id.to_string()));

        if is_factual {
            factual_claims += 1;
            if is_catalog_line && unique_claim {
                traceable_factual_claims += 1;
            }
        }

        let catalog_is_numeric = catalog_claim.is_some_and(|claim| !claim.values.is_empty());
        if catalog_is_numeric || ((is_factual || is_inference) && has_numeric) {
            numeric_claims += 1;
            if !is_catalog_line || !unique_claim {
                unsupported_numeric_claims += 1;
                errors.push("numeric claim is not a unique canonical catalog line".to_string());
            }
        }
        if claim_id.is_some() && (!is_catalog_line || !unique_claim) && !catalog_is_numeric {
            errors.push("claim is not a unique canonical catalog line".to_string());
        }
        if line.contains("[趋势]")
            && !catalog_claim.is_some_and(|claim| claim.kind == "trend" && is_catalog_line)
        {
            errors.push("trend line is not a canonical trend catalog claim".to_string());
        }
        if is_inference {
            inference_claims += 1;
            let references = evidence_bundle_references(line);
            if references_are_valid(&references, &evidence_ids, &bundle_json)
                && references_are_allowlisted(&references)
            {
                traceable_inference_claims += 1;
            } else {
                errors.push("inference is missing valid allowlisted evidence".to_string());
            }
        }
        if line.contains("[建议]") {
            action_claims += 1;
            let references = evidence_bundle_references(line);
            if references_are_valid(&references, &evidence_ids, &bundle_json)
                && references_are_allowlisted(&references)
                && action_is_verifiable(line, bundle.cutoff.date_naive(), &bundle_json)
            {
                verifiable_actions += 1;
            }
        }
    }

    let structure_complete = !errors
        .iter()
        .any(|error| error.starts_with("required section"));
    let factual_traceability_rate = ratio(traceable_factual_claims, factual_claims);
    let unsupported_number_rate = ratio(unsupported_numeric_claims, numeric_claims);
    let inference_traceability_rate = ratio(traceable_inference_claims, inference_claims);
    let action_verifiability_rate = ratio(verifiable_actions, action_claims);
    if factual_claims == 0 {
        errors.push("no [事实] claims found".to_string());
    } else if factual_traceability_rate < 1.0 {
        errors.push(format!(
            "factual traceability rate {factual_traceability_rate:.3} is below 1.000"
        ));
    }
    if unsupported_number_rate > 0.0 {
        errors.push(format!(
            "unsupported number rate {unsupported_number_rate:.3} is above 0.000"
        ));
    }
    if inference_claims > 0 && inference_traceability_rate < 1.0 {
        errors.push(format!(
            "inference traceability rate {inference_traceability_rate:.3} is below 1.000"
        ));
    }
    if action_claims == 0 {
        errors.push("no [建议] claims found".to_string());
    } else if action_verifiability_rate < 1.0 {
        errors.push(format!(
            "action verifiability rate {action_verifiability_rate:.3} is below 1.000"
        ));
    }
    let (novelty_rate, repetition_rate) = previous_portrait
        .filter(|_| !previous_oversized)
        .map(|previous| {
            let novelty = novelty_rate(candidate, previous);
            (Some(novelty), Some(1.0 - novelty))
        })
        .unwrap_or((None, None));
    if novelty_rate.is_some_and(|rate| rate < DEFAULT_NOVELTY_RATE) {
        errors.push(format!(
            "novelty rate {:.3} is below {DEFAULT_NOVELTY_RATE:.3}",
            novelty_rate.unwrap_or_default()
        ));
    }

    PortraitQualityReport {
        gate_version: PORTRAIT_QUALITY_GATE_VERSION.to_string(),
        passed: errors.is_empty(),
        factual_claims,
        traceable_factual_claims,
        factual_traceability_rate,
        numeric_claims,
        unsupported_numeric_claims,
        unsupported_number_rate,
        inference_claims,
        traceable_inference_claims,
        inference_traceability_rate,
        comparable_cohort_rate: f64::from(bundle.comparison.comparable),
        comparison_claims_suppressed: bundle.comparison.comparable && errors.is_empty(),
        action_claims,
        verifiable_actions,
        action_verifiability_rate,
        novelty_rate,
        repetition_rate,
        structure_complete,
        errors,
    }
}

fn failed_quality_report(error: String) -> PortraitQualityReport {
    PortraitQualityReport {
        gate_version: PORTRAIT_QUALITY_GATE_VERSION.to_string(),
        passed: false,
        factual_claims: 0,
        traceable_factual_claims: 0,
        factual_traceability_rate: 0.0,
        numeric_claims: 0,
        unsupported_numeric_claims: 0,
        unsupported_number_rate: 0.0,
        inference_claims: 0,
        traceable_inference_claims: 0,
        inference_traceability_rate: 0.0,
        comparable_cohort_rate: 0.0,
        comparison_claims_suppressed: false,
        action_claims: 0,
        verifiable_actions: 0,
        action_verifiability_rate: 0.0,
        novelty_rate: None,
        repetition_rate: None,
        structure_complete: false,
        errors: vec![error],
    }
}

fn paragraph_blocks(blocks: &[VisibleBlock]) -> impl Iterator<Item = &VisibleBlock> {
    blocks
        .iter()
        .filter(|block| block.kind == VisibleBlockKind::Paragraph)
}

fn validate_structure(blocks: &[VisibleBlock]) -> Vec<String> {
    const REQUIRED: &[&str] = &[
        "L1：认知演进",
        "L2：战略定位",
        "L3：工作方式健康度",
        "L4：成长处方",
    ];
    let mut errors = Vec::new();
    let mut positions = Vec::new();
    for required in REQUIRED {
        let matches: Vec<usize> = blocks
            .iter()
            .enumerate()
            .filter_map(|(index, block)| {
                (block.kind == VisibleBlockKind::Heading(2) && block.text == *required)
                    .then_some(index)
            })
            .collect();
        if matches.len() != 1 {
            errors.push(format!(
                "required section {required:?} must appear exactly once"
            ));
        } else {
            positions.push(matches[0]);
        }
    }
    if positions.len() == REQUIRED.len() && !positions.windows(2).all(|pair| pair[0] < pair[1]) {
        errors.push("required sections are out of order".to_string());
    }
    if positions.len() == REQUIRED.len() {
        for (index, start) in positions.iter().enumerate() {
            let end = positions.get(index + 1).copied().unwrap_or(blocks.len());
            if !blocks[*start + 1..end]
                .iter()
                .any(|block| block.kind == VisibleBlockKind::Paragraph)
            {
                errors.push(format!(
                    "required section {:?} has no substantive content",
                    REQUIRED[index]
                ));
            }
        }
    }
    errors
}

#[derive(Debug)]
enum EvidenceReference {
    Evidence(String),
    Bundle(String),
}

fn evidence_bundle_references(line: &str) -> Vec<EvidenceReference> {
    let mut references = Vec::new();
    for (prefix, evidence) in [("[evidence:", true), ("[bundle:", false)] {
        let mut remainder = line;
        while let Some(start) = remainder.find(prefix) {
            let value_start = start + prefix.len();
            let Some(end) = remainder[value_start..].find(']') else {
                break;
            };
            let value = remainder[value_start..value_start + end].trim();
            if !value.is_empty() {
                references.push(if evidence {
                    EvidenceReference::Evidence(value.to_string())
                } else {
                    EvidenceReference::Bundle(value.to_string())
                });
            }
            remainder = &remainder[value_start + end + 1..];
        }
    }
    references
}

fn references_are_valid(
    references: &[EvidenceReference],
    evidence_ids: &HashSet<&str>,
    bundle_json: &Value,
) -> bool {
    !references.is_empty()
        && references.iter().all(|reference| match reference {
            EvidenceReference::Evidence(id) => evidence_ids.contains(id.as_str()),
            EvidenceReference::Bundle(pointer) => bundle_json.pointer(pointer).is_some(),
        })
}

fn references_are_allowlisted(references: &[EvidenceReference]) -> bool {
    !references.is_empty()
        && references.iter().all(|reference| match reference {
            EvidenceReference::Evidence(_) => true,
            EvidenceReference::Bundle(pointer) => {
                pointer.starts_with("/current/")
                    || pointer.starts_with("/previous/")
                    || pointer.starts_with("/comparison/")
                    || pointer.starts_with("/manifest/current_window/")
                    || pointer.starts_with("/manifest/previous_window/")
            }
        })
}

fn field_value<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("[{field}:");
    line.find(&prefix).and_then(|start| {
        let value = &line[start + prefix.len()..];
        value.find(']').and_then(|end| {
            let value = value[..end].trim();
            let remainder = &line[start + prefix.len() + end + 1..];
            (!value.is_empty() && !remainder.contains(&prefix)).then_some(value)
        })
    })
}

fn action_is_verifiable(line: &str, cutoff: NaiveDate, bundle_json: &Value) -> bool {
    let owner = field_value(line, "owner").unwrap_or_default();
    let due = field_value(line, "due").unwrap_or_default();
    let verify = field_value(line, "verify").unwrap_or_default();
    let owner_valid = owner.chars().count() >= 2
        && owner
            .chars()
            .all(|character| character.is_alphanumeric() || "_-./@".contains(character))
        && !matches!(
            owner.to_ascii_lowercase().as_str(),
            "tbd" | "todo" | "n/a" | "na"
        )
        && owner != "待定";
    let latest_due = cutoff.checked_add_signed(Duration::days(90));
    let due_valid = NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok_and(|date| {
        date.format("%Y-%m-%d").to_string() == due
            && date >= cutoff
            && latest_due.is_some_and(|latest| date <= latest)
    });
    owner_valid && due_valid && typed_verification_is_valid(verify, bundle_json)
}

fn typed_verification_is_valid(value: &str, bundle_json: &Value) -> bool {
    let parts: Vec<&str> = value.split('|').map(str::trim).collect();
    match parts.as_slice() {
        ["metric", pointer, comparator, expected_raw] => {
            let Some(actual) = bundle_json.pointer(pointer) else {
                return false;
            };
            if !verification_pointer_allowed(pointer) {
                return false;
            }
            let Ok(expected) = serde_json::from_str::<Value>(expected_raw) else {
                return false;
            };
            if serde_json::to_string(&expected).ok().as_deref() != Some(*expected_raw) {
                return false;
            }
            match (actual, &expected) {
                (Value::Number(_), Value::Number(_)) => {
                    matches!(*comparator, "eq" | "gt" | "gte" | "lt" | "lte")
                }
                (Value::String(_), Value::String(_)) | (Value::Bool(_), Value::Bool(_)) => {
                    *comparator == "eq"
                }
                _ => false,
            }
        }
        ["artifact", name, state] => {
            valid_verification_name(name) && matches!(*state, "present" | "absent")
        }
        ["check", name, state] => {
            valid_verification_name(name) && matches!(*state, "pass" | "fail")
        }
        _ => false,
    }
}

fn verification_pointer_allowed(pointer: &str) -> bool {
    pointer.starts_with("/current/metrics/")
        || pointer.starts_with("/previous/metrics/")
        || pointer == "/comparison/status"
        || pointer == "/comparison/comparable"
        || (pointer.ends_with("/status")
            && (pointer.starts_with("/manifest/current_window/")
                || pointer.starts_with("/manifest/previous_window/")))
}

fn valid_verification_name(name: &str) -> bool {
    name.chars().count() >= 3
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-./".contains(character))
}

fn contains_numeric_token(line: &str) -> bool {
    let characters: Vec<char> = line.chars().collect();
    let mut rendered = String::with_capacity(line.len());
    let mut index = 0usize;
    while index < characters.len() {
        if characters[index] == '[' {
            if let Some(end) = characters[index + 1..]
                .iter()
                .position(|character| *character == ']')
            {
                let metadata: String = characters[index + 1..index + end + 1].iter().collect();
                if is_machine_field(&metadata) {
                    index += end + 2;
                    continue;
                }
            }
        }
        rendered.push(characters[index]);
        index += 1;
    }
    rendered.chars().any(|character| {
        character.is_ascii_digit()
            || ('０'..='９').contains(&character)
            || "零〇一二三四五六七八九十百千万亿两壹贰叁肆伍陆柒捌玖拾佰仟".contains(character)
    }) || rendered.contains("百分之")
}

fn is_machine_field(value: &str) -> bool {
    value == "事实"
        || value == "建议"
        || value == "趋势"
        || value.starts_with("推断")
        || value.starts_with("claim:")
        || value.starts_with("evidence:")
        || value.starts_with("bundle:")
        || value.starts_with("owner:")
        || value.starts_with("due:")
        || value.starts_with("verify:")
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
