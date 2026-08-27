use super::bundle::CognitivePortraitBundle;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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
    pub comparable_cohort_rate: f64,
    pub comparison_claims_suppressed: bool,
    pub action_claims: usize,
    pub verifiable_actions: usize,
    pub action_verifiability_rate: f64,
    pub novelty_rate: Option<f64>,
    pub repetition_rate: Option<f64>,
    pub errors: Vec<String>,
}

pub(crate) fn validate_portrait(
    bundle: &CognitivePortraitBundle,
    candidate: &str,
    previous_portrait: Option<&str>,
) -> PortraitQualityReport {
    let evidence_ids: HashSet<&str> = bundle
        .current
        .evidence
        .iter()
        .chain(bundle.previous.evidence.iter())
        .map(|evidence| evidence.evidence_id.as_str())
        .collect();
    let bundle_json = serde_json::to_value(bundle).expect("portrait bundle must serialize");
    let mut factual_claims = 0usize;
    let mut traceable_factual_claims = 0usize;
    let mut numeric_claims = 0usize;
    let mut unsupported_numeric_claims = 0usize;
    let mut action_claims = 0usize;
    let mut verifiable_actions = 0usize;

    for line in candidate
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let references = claim_references(line);
        let references_valid = !references.is_empty()
            && references.iter().all(|reference| match reference {
                ClaimReference::Evidence(id) => evidence_ids.contains(id.as_str()),
                ClaimReference::Bundle(pointer) => bundle_json.pointer(pointer).is_some(),
            });
        if line.contains("[事实]") {
            factual_claims += 1;
            if references_valid {
                traceable_factual_claims += 1;
            }
        }
        if (line.contains("[事实]") || line.contains("[推断"))
            && line.chars().any(|character| character.is_ascii_digit())
        {
            numeric_claims += 1;
            if !references_valid {
                unsupported_numeric_claims += 1;
            }
        }
        if line.contains("[建议]") {
            action_claims += 1;
            if references_valid
                && required_field(line, "owner")
                && required_field(line, "due")
                && required_field(line, "verify")
            {
                verifiable_actions += 1;
            }
        }
    }

    let factual_traceability_rate = ratio(traceable_factual_claims, factual_claims);
    let unsupported_number_rate = ratio(unsupported_numeric_claims, numeric_claims);
    let action_verifiability_rate = ratio(verifiable_actions, action_claims);
    let comparison_claims_suppressed =
        bundle.comparison.comparable || !contains_trend_claim(candidate);
    let (novelty_rate, repetition_rate) = previous_portrait
        .map(|previous| {
            let novelty = novelty_rate(candidate, previous);
            (Some(novelty), Some(1.0 - novelty))
        })
        .unwrap_or((None, None));
    let mut errors = Vec::new();
    if factual_claims == 0 {
        errors.push("no [事实] claims found".to_string());
    } else if factual_traceability_rate < 1.0 {
        errors.push(format!(
            "factual traceability rate {:.3} is below 1.000",
            factual_traceability_rate
        ));
    }
    if unsupported_number_rate > 0.0 {
        errors.push(format!(
            "unsupported number rate {:.3} is above 0.000",
            unsupported_number_rate
        ));
    }
    if !comparison_claims_suppressed {
        errors.push("comparison is DEGRADED but candidate contains trend claims".to_string());
    }
    if action_claims == 0 {
        errors.push("no [建议] claims found".to_string());
    } else if action_verifiability_rate < 1.0 {
        errors.push(format!(
            "action verifiability rate {:.3} is below 1.000",
            action_verifiability_rate
        ));
    }
    if novelty_rate.is_some_and(|rate| rate < DEFAULT_NOVELTY_RATE) {
        errors.push(format!(
            "novelty rate {:.3} is below {:.3}",
            novelty_rate.unwrap_or_default(),
            DEFAULT_NOVELTY_RATE
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
        comparable_cohort_rate: if bundle.comparison.comparable {
            1.0
        } else {
            0.0
        },
        comparison_claims_suppressed,
        action_claims,
        verifiable_actions,
        action_verifiability_rate,
        novelty_rate,
        repetition_rate,
        errors,
    }
}

#[derive(Debug)]
enum ClaimReference {
    Evidence(String),
    Bundle(String),
}

fn claim_references(line: &str) -> Vec<ClaimReference> {
    let mut references = Vec::new();
    for (prefix, bundle_reference) in [("[evidence:", false), ("[bundle:", true)] {
        let mut remainder = line;
        while let Some(start) = remainder.find(prefix) {
            let value_start = start + prefix.len();
            let Some(end) = remainder[value_start..].find(']') else {
                break;
            };
            let value = remainder[value_start..value_start + end].trim();
            if !value.is_empty() {
                references.push(if bundle_reference {
                    ClaimReference::Bundle(value.to_string())
                } else {
                    ClaimReference::Evidence(value.to_string())
                });
            }
            remainder = &remainder[value_start + end + 1..];
        }
    }
    references
}

fn required_field(line: &str, field: &str) -> bool {
    let prefix = format!("[{field}:");
    line.find(&prefix).is_some_and(|start| {
        let value = &line[start + prefix.len()..];
        value
            .find(']')
            .is_some_and(|end| !value[..end].trim().is_empty())
    })
}

fn contains_trend_claim(candidate: &str) -> bool {
    const TREND_MARKERS: &[&str] = &[
        "[趋势]", "→", "同比", "环比", "较上期", "上升", "下降", "增加", "减少", "反转",
    ];
    candidate.lines().any(|line| {
        (line.contains("[事实]") || line.contains("[推断"))
            && TREND_MARKERS.iter().any(|marker| line.contains(marker))
    })
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn novelty_rate(candidate: &str, previous: &str) -> f64 {
    let candidate = normalized_paragraphs(candidate);
    if candidate.is_empty() {
        return 0.0;
    }
    let previous: HashSet<String> = normalized_paragraphs(previous).into_iter().collect();
    let novel = candidate
        .iter()
        .filter(|paragraph| !previous.contains(*paragraph))
        .count();
    ratio(novel, candidate.len())
}

fn normalized_paragraphs(report: &str) -> Vec<String> {
    report
        .split("\n\n")
        .filter_map(|paragraph| {
            let trimmed = paragraph.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("---")
                || trimmed.starts_with('|')
            {
                return None;
            }
            let normalized: String = trimmed
                .chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            (normalized.chars().count() >= 20).then_some(normalized)
        })
        .collect()
}
