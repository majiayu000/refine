#![allow(dead_code)]

#[path = "../src/cognitive_portrait_data/mod.rs"]
mod cognitive_portrait_data;
#[path = "../src/insights_manifest.rs"]
mod insights_manifest;

use chrono::{DateTime, Duration, TimeZone, Utc};
use cognitive_portrait_data::{
    read_bundle, validate_portrait, write_bundle, CognitivePortraitBundle,
    PORTRAIT_BUNDLE_SCHEMA_VERSION, PORTRAIT_COLLECTOR_VERSION,
};
use refine_core::knowledge::{
    DocumentId, Item, ItemId, ItemType, ObservationDocumentMeta, ObservationWindowSnapshot,
    RestoreParams, Tag,
};

fn observation(
    id: &str,
    document_id: Option<&str>,
    created_at: DateTime<Utc>,
    tags: &[&str],
    content: &str,
) -> Item {
    Item::restore(RestoreParams {
        id: ItemId::from(id),
        item_type: ItemType::Observation,
        title: format!("title {id}"),
        summary: format!("summary {id}"),
        content: content.to_string(),
        tags: tags.iter().map(|tag| Tag::new(tag).unwrap()).collect(),
        source: None,
        document_id: document_id.map(DocumentId::from),
        excerpt: Some(format!("excerpt {id}")),
        created_at,
        updated_at: created_at,
    })
    .unwrap()
}

fn fixture(include_unsupported: bool) -> CognitivePortraitBundle {
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let current_time = cutoff - Duration::days(1);
    let previous_time = cutoff - Duration::days(91);
    let mut current = vec![observation(
        "current",
        Some("current-doc"),
        cutoff,
        &["refine", "decision", "competent"],
        "知识:\n- cohort integrity\n阻力:\n- stale data",
    )];
    let mut documents = vec![ObservationDocumentMeta {
        id: DocumentId::from("current-doc"),
        source: "codex-session".into(),
        captured_at: current_time,
    }];
    if include_unsupported {
        current.push(observation(
            "grok",
            Some("grok-doc"),
            current_time,
            &["refine"],
            "知识:\n- legacy note",
        ));
        documents.push(ObservationDocumentMeta {
            id: DocumentId::from("grok-doc"),
            source: "grok-knowledge".into(),
            captured_at: current_time,
        });
    }
    let previous = vec![observation(
        "previous",
        Some("previous-doc"),
        previous_time,
        &["refine", "bugfix", "delegation"],
        "模式:\n- verify before merge",
    )];
    documents.push(ObservationDocumentMeta {
        id: DocumentId::from("previous-doc"),
        source: "claude-code-session".into(),
        captured_at: previous_time,
    });
    cognitive_portrait_data::build_bundle_from_snapshot(
        ObservationWindowSnapshot {
            current,
            previous,
            documents,
        },
        cutoff,
        90,
    )
    .unwrap()
}

#[test]
fn collector_reuses_source_cohort_and_emits_traceable_evidence() {
    let bundle = fixture(false);
    assert_eq!(bundle.current.metrics.total_sessions, 1);
    assert_eq!(bundle.current.metrics.total_decisions, 1);
    assert_eq!(bundle.previous.metrics.total_bugfixes, 1);
    assert_eq!(bundle.current.evidence[0].evidence_id, "obs:current");
    assert_eq!(bundle.current.evidence[0].source, "codex");
    assert_eq!(bundle.previous.evidence[0].source, "claude");
    assert!(bundle.comparison.comparable);
}

#[test]
fn unsupported_knowledge_source_is_disclosed_and_suppresses_trends() {
    let bundle = fixture(true);
    assert_eq!(bundle.current.metrics.total_sessions, 1);
    assert_eq!(bundle.current.evidence.len(), 1);
    assert_eq!(
        bundle.manifest.current_window.unsupported_source_counts[0].source,
        "grok-knowledge"
    );
    assert!(!bundle.comparison.comparable);
    assert_eq!(bundle.comparison.status, "DEGRADED");
}

#[test]
fn empty_core_data_fails_clearly() {
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let error = cognitive_portrait_data::build_bundle_from_snapshot(
        ObservationWindowSnapshot {
            current: Vec::new(),
            previous: Vec::new(),
            documents: Vec::new(),
        },
        cutoff,
        90,
    )
    .unwrap_err();
    assert!(error.to_string().contains("NO_CORE_DATA"));
}

#[test]
fn bundle_round_trip_is_versioned_and_deterministic() {
    let bundle = fixture(false);
    let directory = tempfile::tempdir().unwrap();
    let first = directory.path().join("first.json");
    let second = directory.path().join("second.json");
    write_bundle(&first, &bundle).unwrap();
    write_bundle(&second, &bundle).unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let restored = read_bundle(&first).unwrap();
    assert_eq!(restored, bundle);
    assert_eq!(restored.schema_version, PORTRAIT_BUNDLE_SCHEMA_VERSION);
    assert_eq!(restored.collector_version, PORTRAIT_COLLECTOR_VERSION);
}

#[test]
fn quality_gate_requires_evidence_numbers_novelty_and_verifiable_actions() {
    let bundle = fixture(false);
    let candidate = "# Portrait\n\n[事实] 当前窗口有 1 个 session。[bundle:/current/metrics/total_sessions]\n\n[建议] 在截止日前检查 cohort。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:重新运行 collector]\n\n这是一段足够长且全新的认知分析内容，用于证明本期相对上一期具备真实新增信息。";
    let previous = "# Portrait\n\n这是一段足够长但完全不同的旧版认知分析内容，用于建立上一期基线。";
    let report = validate_portrait(&bundle, candidate, Some(previous));
    assert!(report.passed, "{:?}", report.errors);
    assert_eq!(report.factual_traceability_rate, 1.0);
    assert_eq!(report.unsupported_number_rate, 0.0);
    assert_eq!(report.action_verifiability_rate, 1.0);

    let bad = "[事实] 当前窗口有 9 个 session。\n\n[建议] 继续做。";
    let report = validate_portrait(&bundle, bad, Some(bad));
    assert!(!report.passed);
    assert!(report.unsupported_number_rate > 0.0);
    assert!(report.action_verifiability_rate < 1.0);
    assert_eq!(report.novelty_rate, Some(0.0));
}

#[test]
fn degraded_comparison_rejects_trend_claims_but_allows_explicit_suppression() {
    let bundle = fixture(true);
    let trend = "[事实][趋势] session 1→2。[bundle:/current/metrics/total_sessions]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:修复数据后重跑]";
    assert!(!validate_portrait(&bundle, trend, None).passed);
    let prose_trend = "[推断，置信度：高] session 较上期上升。[bundle:/comparison/status]\n\n[事实] 当前比较状态不可用。[bundle:/comparison/status]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:修复数据后重跑]";
    assert!(!validate_portrait(&bundle, prose_trend, None).passed);
    let suppressed = "[事实] 当前不可比较。[bundle:/comparison/status]\n\n[建议] 暂停跨期判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:修复数据后重跑]";
    assert!(validate_portrait(&bundle, suppressed, None).passed);
}
