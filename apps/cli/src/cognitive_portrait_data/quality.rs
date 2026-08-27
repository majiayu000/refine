use super::bundle::CognitivePortraitBundle;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

mod markdown;
mod novelty;
use markdown::visible_lines;
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

    let visible = visible_lines(candidate);
    for line in visible.iter().map(|line| line.text.as_str()) {
        let references = claim_references(line);
        let references_valid = references_are_valid(&references, &evidence_ids, &bundle_json);
        if line.contains("[事实]") {
            factual_claims += 1;
            if references_valid {
                traceable_factual_claims += 1;
            }
        }
        let metric_claims = references
            .iter()
            .filter(|reference| matches!(reference, ClaimReference::Metric { .. }))
            .count();
        let has_metric_syntax = line.contains("[metric:");
        if (line.contains("[事实]") || line.contains("[推断"))
            && (has_metric_syntax || contains_unstructured_numeric(line))
        {
            numeric_claims += 1;
            if !references_valid
                || metric_claims == 0
                || contains_unstructured_numeric(line)
                || !metric_claims_are_valid(&references, &bundle_json)
            {
                unsupported_numeric_claims += 1;
            }
        }
        if line.contains("[建议]") {
            action_claims += 1;
            if references_valid
                && action_references_allowed(&references)
                && action_is_verifiable(line, bundle.cutoff.date_naive(), &bundle_json)
            {
                verifiable_actions += 1;
            }
        }
    }

    let factual_traceability_rate = ratio(traceable_factual_claims, factual_claims);
    let unsupported_number_rate = ratio(unsupported_numeric_claims, numeric_claims);
    let action_verifiability_rate = ratio(verifiable_actions, action_claims);
    let trend_errors = validate_trends(bundle, candidate, &evidence_ids, &bundle_json);
    let comparison_claims_suppressed = trend_errors.is_empty();
    let (novelty_rate, repetition_rate) = previous_portrait
        .map(|previous| {
            let novelty = novelty_rate(candidate, previous);
            (Some(novelty), Some(1.0 - novelty))
        })
        .unwrap_or((None, None));
    let mut errors = Vec::new();
    let structure_errors = validate_structure(candidate);
    let structure_complete = structure_errors.is_empty();
    errors.extend(structure_errors);
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
    errors.extend(trend_errors);
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
        structure_complete,
        errors,
    }
}

#[derive(Debug, Clone)]
enum ClaimReference {
    Evidence(String),
    Bundle(String),
    Metric {
        pointer: String,
        value: serde_json::Number,
    },
}

fn references_are_valid(
    references: &[ClaimReference],
    evidence_ids: &HashSet<&str>,
    bundle_json: &Value,
) -> bool {
    !references.is_empty()
        && references.iter().all(|reference| match reference {
            ClaimReference::Evidence(id) => evidence_ids.contains(id.as_str()),
            ClaimReference::Bundle(pointer) => bundle_json.pointer(pointer).is_some(),
            ClaimReference::Metric { pointer, value } => {
                metric_pointer_allowed(pointer)
                    && bundle_json.pointer(pointer) == Some(&Value::Number(value.clone()))
            }
        })
}

fn metric_claims_are_valid(references: &[ClaimReference], bundle_json: &Value) -> bool {
    references.iter().all(|reference| match reference {
        ClaimReference::Metric { pointer, value } => {
            metric_pointer_allowed(pointer)
                && bundle_json.pointer(pointer) == Some(&Value::Number(value.clone()))
        }
        ClaimReference::Evidence(_) | ClaimReference::Bundle(_) => true,
    })
}

fn metric_pointer_allowed(pointer: &str) -> bool {
    if pointer.starts_with("/current/metrics/") || pointer.starts_with("/previous/metrics/") {
        return true;
    }
    let manifest_metric = pointer.starts_with("/manifest/current_window/")
        || pointer.starts_with("/manifest/previous_window/");
    let leaf = pointer.rsplit('/').next().unwrap_or_default();
    manifest_metric
        && matches!(
            leaf,
            "input_observations"
                | "linked_observations"
                | "detached_observations"
                | "mode_excluded_observations"
                | "source_excluded_observations"
                | "eligible_observations"
                | "count"
        )
}

fn contains_unstructured_numeric(line: &str) -> bool {
    let prose = strip_machine_metadata(line);
    let characters: Vec<char> = prose.chars().collect();
    characters.iter().enumerate().any(|(index, character)| {
        character.is_ascii_digit()
            || ('０'..='９').contains(character)
            || "零〇二三四五六七八九十百千万亿两壹贰叁肆伍陆柒捌玖拾佰仟".contains(*character)
            || (*character == '一'
                && characters
                    .get(index + 1)
                    .is_some_and(|next| "个条项次份人天周月年百分".contains(*next)))
    }) || prose.contains("百分之")
}

fn strip_machine_metadata(line: &str) -> String {
    let mut remaining = line;
    let mut output = String::with_capacity(line.len());
    while let Some(start) = remaining.find('[') {
        output.push_str(&remaining[..start]);
        let Some(end) = remaining[start + 1..].find(']') else {
            output.push_str(&remaining[start..]);
            return output;
        };
        let metadata = &remaining[start + 1..start + 1 + end];
        let machine = matches!(metadata, "事实" | "建议" | "趋势" | "趋势抑制")
            || metadata.starts_with("推断")
            || [
                "evidence:",
                "bundle:",
                "metric:",
                "owner:",
                "due:",
                "verify:",
            ]
            .iter()
            .any(|prefix| metadata.starts_with(prefix));
        if !machine {
            output.push_str(&remaining[start..start + end + 2]);
        }
        remaining = &remaining[start + end + 2..];
    }
    output.push_str(remaining);
    output
}

fn claim_references(line: &str) -> Vec<ClaimReference> {
    let mut references = Vec::new();
    for (prefix, reference_type) in [
        ("[evidence:", "evidence"),
        ("[bundle:", "bundle"),
        ("[metric:", "metric"),
    ] {
        let mut remainder = line;
        while let Some(start) = remainder.find(prefix) {
            let value_start = start + prefix.len();
            let Some(end) = remainder[value_start..].find(']') else {
                break;
            };
            let value = remainder[value_start..value_start + end].trim();
            if !value.is_empty() {
                match reference_type {
                    "evidence" => references.push(ClaimReference::Evidence(value.to_string())),
                    "bundle" => references.push(ClaimReference::Bundle(value.to_string())),
                    "metric" => {
                        if let Some((pointer, displayed)) = value.rsplit_once('=') {
                            if let Some(displayed) = parse_canonical_number(displayed.trim()) {
                                references.push(ClaimReference::Metric {
                                    pointer: pointer.trim().to_string(),
                                    value: displayed,
                                });
                            }
                        }
                    }
                    _ => unreachable!("known claim reference type"),
                }
            }
            remainder = &remainder[value_start + end + 1..];
        }
    }
    references
}

fn parse_canonical_number(value: &str) -> Option<serde_json::Number> {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let (integer, fraction) = unsigned
        .split_once('.')
        .map_or((unsigned, None), |(integer, fraction)| {
            (integer, Some(fraction))
        });
    let integer_valid = integer == "0"
        || (!integer.starts_with('0')
            && !integer.is_empty()
            && integer.chars().all(|character| character.is_ascii_digit()));
    let fraction_valid = fraction.is_none_or(|fraction| {
        !fraction.is_empty() && fraction.chars().all(|character| character.is_ascii_digit())
    });
    if !integer_valid || !fraction_valid || value == "-0" {
        return None;
    }
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|value| value.as_number().cloned())
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

fn action_references_allowed(references: &[ClaimReference]) -> bool {
    references.iter().any(|reference| match reference {
        ClaimReference::Evidence(_) | ClaimReference::Metric { .. } => true,
        ClaimReference::Bundle(pointer) => {
            pointer.starts_with("/current/")
                || pointer.starts_with("/previous/")
                || pointer.starts_with("/comparison/")
                || pointer.starts_with("/manifest/current_window/")
                || pointer.starts_with("/manifest/previous_window/")
        }
    })
}

fn action_is_verifiable(line: &str, cutoff: NaiveDate, bundle_json: &Value) -> bool {
    let owner = field_value(line, "owner").unwrap_or_default().trim();
    let due = field_value(line, "due").unwrap_or_default().trim();
    let verify = field_value(line, "verify").unwrap_or_default().trim();
    let owner_valid = owner.chars().count() >= 2
        && owner.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | '/' | '@')
        })
        && !matches!(
            owner.to_ascii_lowercase().as_str(),
            "tbd" | "todo" | "n/a" | "na"
        )
        && owner != "待定";
    let latest_due = cutoff.checked_add_signed(chrono::Duration::days(90));
    let due_valid = NaiveDate::parse_from_str(due, "%Y-%m-%d").is_ok_and(|date| {
        date.format("%Y-%m-%d").to_string() == due
            && date >= cutoff
            && latest_due.is_some_and(|latest| date <= latest)
    });
    owner_valid && due_valid && structured_verification_is_valid(verify, bundle_json)
}

fn structured_verification_is_valid(value: &str, bundle_json: &Value) -> bool {
    let Some((subject, operator, expected)) = split_comparison(value) else {
        return false;
    };
    if expected.is_empty() {
        return false;
    }
    if let Some(pointer) = subject.strip_prefix("metric:") {
        return verification_pointer_allowed(pointer) && bundle_json.pointer(pointer).is_some();
    }
    if let Some(name) = subject.strip_prefix("artifact:") {
        return operator == "=="
            && valid_verification_name(name)
            && matches!(expected, "present" | "absent");
    }
    if let Some(name) = subject.strip_prefix("check:") {
        return operator == "=="
            && valid_verification_name(name)
            && matches!(expected, "pass" | "fail");
    }
    false
}

fn verification_pointer_allowed(pointer: &str) -> bool {
    metric_pointer_allowed(pointer)
        || pointer == "/comparison/status"
        || pointer == "/comparison/comparable"
        || pointer.ends_with("/status")
            && (pointer.starts_with("/manifest/current_window/")
                || pointer.starts_with("/manifest/previous_window/"))
}

fn split_comparison(value: &str) -> Option<(&str, &str, &str)> {
    for operator in ["==", ">=", "<=", ">", "<"] {
        if let Some((subject, expected)) = value.split_once(operator) {
            return Some((subject.trim(), operator, expected.trim()));
        }
    }
    None
}

fn valid_verification_name(name: &str) -> bool {
    name.chars().count() >= 3
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '/')
        })
}

fn validate_structure(candidate: &str) -> Vec<String> {
    const REQUIRED: &[&str] = &[
        "## L1：认知演进",
        "## L2：战略定位",
        "## L3：工作方式健康度",
        "## L4：成长处方",
    ];
    let visible_lines = visible_lines(candidate);
    let mut errors = Vec::new();
    let mut positions = Vec::new();
    for required in REQUIRED {
        let matches: Vec<usize> = visible_lines
            .iter()
            .filter_map(|line| (line.text == *required).then_some(line.source_index))
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
        errors.push("required L1-L4 sections are out of order".to_string());
    }
    if positions.len() == REQUIRED.len() && errors.is_empty() {
        for (section_index, start) in positions.iter().enumerate() {
            let end = positions
                .get(section_index + 1)
                .copied()
                .unwrap_or(usize::MAX);
            let has_content = visible_lines.iter().any(|line| {
                line.source_index > *start
                    && line.source_index < end
                    && !line.text.starts_with('#')
                    && !line.text.starts_with("---")
                    && !line.text.starts_with('|')
            });
            if !has_content {
                errors.push(format!(
                    "required section {:?} has no substantive content",
                    REQUIRED[section_index]
                ));
            }
        }
    }
    errors
}

fn validate_trends(
    bundle: &CognitivePortraitBundle,
    candidate: &str,
    evidence_ids: &HashSet<&str>,
    bundle_json: &Value,
) -> Vec<String> {
    let visible = visible_lines(candidate);
    let claim_lines: Vec<&str> = visible
        .iter()
        .map(|line| line.text.as_str())
        .filter(|line| line.contains("[事实]") || line.contains("[推断"))
        .collect();
    if !bundle.comparison.comparable {
        let suppression_is_explicit = claim_lines.iter().any(|line| {
            line.contains("[事实]")
                && line.contains("[趋势抑制]")
                && line.contains("[bundle:/comparison/status]")
                && references_are_valid(&claim_references(line), evidence_ids, bundle_json)
        });
        let mut errors = Vec::new();
        let forbidden_comparison = claim_lines.iter().any(|line| {
            if line.contains("[趋势抑制]") {
                return false;
            }
            let references = claim_references(line);
            is_trend_claim(line)
                || contains_cross_period_anchor(line)
                || references.iter().any(|reference| {
                    matches!(reference, ClaimReference::Bundle(pointer) if pointer.starts_with("/previous/") || pointer.starts_with("/comparison/"))
                        || matches!(reference, ClaimReference::Metric { pointer, .. } if pointer.starts_with("/previous/"))
                })
        });
        if forbidden_comparison {
            errors.push("comparison is DEGRADED but candidate contains trend claims".to_string());
        }
        if !suppression_is_explicit {
            errors.push(
                "comparison is DEGRADED but candidate lacks an explicit [趋势抑制] claim"
                    .to_string(),
            );
        }
        return errors;
    }

    claim_lines
        .into_iter()
        .filter(|line| is_trend_claim(line) || contains_cross_period_anchor(line))
        .filter_map(|line| {
            let references = claim_references(line);
            let has_current = references.iter().any(|reference| {
                matches!(reference, ClaimReference::Metric { pointer, .. } if pointer.starts_with("/current/"))
            });
            let has_previous = references.iter().any(|reference| {
                matches!(reference, ClaimReference::Metric { pointer, .. } if pointer.starts_with("/previous/"))
            });
            (!line.contains("[趋势]")
                || !(line.contains("[事实]") || line.contains("[推断"))
                || !has_current
                || !has_previous
                || !references_are_valid(&references, evidence_ids, bundle_json)
                || !metric_claims_are_valid(&references, bundle_json))
            .then(|| {
                "comparable trend claims require [趋势] plus valid current and previous bundle scalars"
                    .to_string()
            })
        })
        .collect()
}

fn contains_cross_period_anchor(line: &str) -> bool {
    const ANCHORS: &[&str] = &[
        "上期",
        "前期",
        "上一期",
        "前一期",
        "前一窗口",
        "上一窗口",
        "前个窗口",
        "此前",
        "以往",
        "历史同期",
        "previous",
        "prior",
        "last period",
        "earlier window",
        "previous window",
        "prior window",
    ];
    let lower = line.to_ascii_lowercase();
    ANCHORS.iter().any(|anchor| lower.contains(anchor))
}

fn is_trend_claim(line: &str) -> bool {
    const TREND_MARKERS: &[&str] = &[
        "[趋势]",
        "→",
        "同比",
        "环比",
        "较上期",
        "上升",
        "下降",
        "增加",
        "减少",
        "反转",
        "相比",
        "相较",
        "比上期",
        "比前期",
        "更高",
        "更低",
        "更多",
        "更少",
        "增强",
        "减弱",
        "增长",
        "降低",
        "下滑",
        "提升",
        "收缩",
        "扩大",
        "改善",
        "恶化",
        "超过",
        "少于",
        "多于",
        "领先",
        "落后",
        "grew",
        "grown",
        "growth",
        "increase",
        "increased",
        "decrease",
        "decreased",
        "decline",
        "declined",
        "improve",
        "improved",
        "worse",
        "better",
        "higher",
        "lower",
        "more than",
        "less than",
        "versus",
        "vs.",
    ];
    let lower = line.to_ascii_lowercase();
    (line.contains("[事实]") || line.contains("[推断") || line.contains("[趋势]"))
        && TREND_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}
