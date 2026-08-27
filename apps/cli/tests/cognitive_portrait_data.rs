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
    observation_with_title(
        id,
        &format!("title {id}"),
        document_id,
        created_at,
        tags,
        content,
    )
}

fn observation_with_title(
    id: &str,
    title: &str,
    document_id: Option<&str>,
    created_at: DateTime<Utc>,
    tags: &[&str],
    content: &str,
) -> Item {
    Item::restore(RestoreParams {
        id: ItemId::from(id),
        item_type: ItemType::Observation,
        title: title.to_string(),
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

fn portrait(body: &str) -> String {
    format!(
        "# Cognitive Portrait\n\n## L1：认知演进\n\n{body}\n\n## L2：战略定位\n\n本层记录当前项目边界和来源覆盖。\n\n## L3：工作方式健康度\n\n本层记录当前摩擦、协作与知识沉淀。\n\n## L4：成长处方\n\n本层汇总上述证据支持的可验证行动。\n"
    )
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

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["comparison"]["status"] = serde_json::json!("DEGRADED");
    let invalid = directory.path().join("invalid.json");
    std::fs::write(&invalid, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(read_bundle(&invalid)
        .unwrap_err()
        .to_string()
        .contains("comparison contract"));
}

#[test]
fn quality_gate_requires_evidence_numbers_novelty_and_verifiable_actions() {
    let bundle = fixture(false);
    let candidate = portrait("[事实] 当前窗口 session 总量见机器指标。[metric:/current/metrics/total_sessions=1]\n\n[建议] 在截止日前检查 cohort。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]\n\n这是一段足够长且全新的认知分析内容，用于证明本期相对上一期具备真实新增信息。");
    let previous = "# Portrait\n\n这是一段足够长但完全不同的旧版认知分析内容，用于建立上一期基线。";
    let report = validate_portrait(&bundle, &candidate, Some(previous));
    assert!(report.passed, "{:?}", report.errors);
    assert_eq!(report.factual_traceability_rate, 1.0);
    assert_eq!(report.unsupported_number_rate, 0.0);
    assert_eq!(report.action_verifiability_rate, 1.0);

    let bad = portrait("[事实] 当前窗口有 9 个 session。\n\n[建议] 继续做。");
    let report = validate_portrait(&bundle, &bad, Some(&bad));
    assert!(!report.passed);
    assert!(report.unsupported_number_rate > 0.0);
    assert!(report.action_verifiability_rate < 1.0);
    assert_eq!(report.novelty_rate, Some(0.0));
}

#[test]
fn degraded_comparison_rejects_trend_claims_but_allows_explicit_suppression() {
    let bundle = fixture(true);
    let trend = portrait("[事实][趋势] session 指标跨窗口变化。[metric:/current/metrics/total_sessions=1]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(!validate_portrait(&bundle, &trend, None).passed);
    let prose_trend = portrait("[推断，置信度：高] session 相较前期呈更强态势。[bundle:/comparison/status]\n\n[事实] 当前比较状态不可用。[bundle:/comparison/status]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(!validate_portrait(&bundle, &prose_trend, None).passed);
    let english_trend = portrait("[事实] Session concentration outperformed the previous window。[bundle:/comparison/status]\n\n[事实][趋势抑制] 当前不可比较。[bundle:/comparison/status]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(!validate_portrait(&bundle, &english_trend, None).passed);
    let chinese_bypass = portrait("[事实] 本期表现优于前一窗口。[bundle:/comparison/status]\n\n[事实][趋势抑制] 当前不可比较。[bundle:/comparison/status]\n\n[建议] 暂停趋势判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(!validate_portrait(&bundle, &chinese_bypass, None).passed);
    let suppressed = portrait("[事实][趋势抑制] 当前不可比较。[bundle:/comparison/status]\n\n[建议] 暂停跨期判断。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(validate_portrait(&bundle, &suppressed, None).passed);
}

#[test]
fn comparable_trends_require_explicit_marker_and_both_window_scalars() {
    let bundle = fixture(false);
    let valid = portrait("[事实][趋势] session 总量在当前与上一窗口保持一致。[metric:/current/metrics/total_sessions=1] [metric:/previous/metrics/total_sessions=1]\n\n[建议] 保持观察。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    let report = validate_portrait(&bundle, &valid, None);
    assert!(report.passed, "{:?}", report.errors);

    let implicit = portrait("[事实] session 总量较上期持平。[metric:/current/metrics/total_sessions=1] [metric:/previous/metrics/total_sessions=1]\n\n[建议] 保持观察。[bundle:/comparison/status] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    assert!(!validate_portrait(&bundle, &implicit, None).passed);
}

#[test]
fn numeric_claim_must_equal_its_referenced_scalar() {
    let bundle = fixture(false);
    for claim in [
        "[事实] 当前窗口有 1 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口有 1e1 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口有 -1 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口有 1% session。[bundle:/schema_version]",
        "[事实] 当前窗口有 1,000 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口有 １ 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口有一百个 session。[bundle:/schema_version]",
        "[事实] 当前窗口 session 总量见指标。[metric:/schema_version=1]",
        "[事实] 当前窗口 session 总量见指标。[metric:/current/metrics/total_sessions=1e0]",
        "[事实] 当前窗口 session 总量见指标。[metric:/current/metrics/total_sessions=+1]",
        "[事实] 当前窗口 session 总量见指标。[metric:/current/metrics/total_sessions=01]",
    ] {
        let candidate = portrait(&format!("{claim}\n\n[建议] 核对统计。[bundle:/current/metrics/total_sessions] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]"));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "claim unexpectedly passed: {claim}");
        assert_eq!(report.unsupported_numeric_claims, 1, "claim: {claim}");
    }
}

#[test]
fn required_sections_due_date_and_observable_verification_fail_closed() {
    let bundle = fixture(false);
    let candidate = "## L1：认知演进\n\n[事实] 当前窗口有 1 个 session。[bundle:/current/metrics/total_sessions]\n\n[建议] 稍后处理。[evidence:obs:current] [owner:TBD] [due:2026-02-30] [verify:重跑]";
    let report = validate_portrait(&bundle, candidate, None);
    assert!(!report.structure_complete);
    assert_eq!(report.action_verifiability_rate, 0.0);
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("required section")));
}

#[test]
fn html_nonce_does_not_create_false_novelty() {
    let bundle = fixture(false);
    let previous = portrait("[事实] 当前窗口 session 总量见指标。[metric:/current/metrics/total_sessions=1]\n\n[建议] 核对统计。[bundle:/current/metrics/total_sessions] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]\n\n[可见标签](https://old.example) 这一段分析文字在两期完全相同，只修改隐藏元数据也不能算作新增洞察。\n\n[ref]: https://old.example");
    let candidate = previous
        .replace("https://old.example", "https://new.example")
        .replace(
            "这一段分析文字",
            "<span data-nonce=\"random-999999\"></span>这一段分析文字",
        );
    let report = validate_portrait(&bundle, &candidate, Some(&previous));
    assert!(!report.passed);
    assert_eq!(report.novelty_rate, Some(0.0));
}

#[test]
fn fenced_or_commented_report_content_is_not_a_portrait() {
    let bundle = fixture(false);
    let hidden = portrait("[事实] 当前窗口 session 总量见指标。[metric:/current/metrics/total_sessions=1]\n\n[建议] 核对统计。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric:/comparison/status==OK]");
    let fenced = format!("~~~markdown\n{hidden}~~~\n<!-- ## L1：认知演进 -->");
    let report = validate_portrait(&bundle, &fenced, None);
    assert!(!report.passed);
    assert!(!report.structure_complete);
    assert_eq!(report.factual_claims, 0);
    assert_eq!(report.action_claims, 0);
}

#[test]
fn action_contract_rejects_placeholder_owner_unbounded_due_and_prose_verify() {
    let bundle = fixture(false);
    let candidate = portrait("[事实] 当前窗口状态已采集。[bundle:/comparison/status]\n\n[建议] 随便处理。[bundle:/schema_version] [owner:x] [due:9999-12-31] [verify:以后再说因为情况未知]");
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(!report.passed);
    assert_eq!(report.verifiable_actions, 0);

    let metadata_verify = portrait("[事实] 当前窗口状态已采集。[bundle:/comparison/status]\n\n[建议] 核对统计。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric:/schema_version==1]");
    let report = validate_portrait(&bundle, &metadata_verify, None);
    assert!(!report.passed);
    assert_eq!(report.verifiable_actions, 0);
}

#[test]
fn duplicate_titles_keep_direct_cross_project_assignment_and_stable_ties() {
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let alpha = observation_with_title(
        "alpha-item",
        "Shared title",
        Some("alpha-doc"),
        cutoff,
        &["alpha", "decision"],
        "知识:\n- alpha",
    );
    let zeta = observation_with_title(
        "zeta-item",
        "Shared title",
        Some("zeta-doc"),
        cutoff,
        &["zeta", "decision"],
        "知识:\n- zeta",
    );
    let previous_time = cutoff - Duration::days(91);
    let bundle = cognitive_portrait_data::build_bundle_from_snapshot(
        ObservationWindowSnapshot {
            current: vec![zeta, alpha],
            previous: vec![observation(
                "previous",
                Some("previous-doc"),
                previous_time,
                &["previous"],
                "知识:\n- previous",
            )],
            documents: vec![
                ObservationDocumentMeta {
                    id: DocumentId::from("alpha-doc"),
                    source: "codex-session".into(),
                    captured_at: cutoff - Duration::days(1),
                },
                ObservationDocumentMeta {
                    id: DocumentId::from("zeta-doc"),
                    source: "codex-session".into(),
                    captured_at: cutoff - Duration::days(1),
                },
                ObservationDocumentMeta {
                    id: DocumentId::from("previous-doc"),
                    source: "claude-code-session".into(),
                    captured_at: previous_time,
                },
            ],
        },
        cutoff,
        90,
    )
    .unwrap();
    let projects: std::collections::BTreeMap<_, _> = bundle
        .current
        .evidence
        .iter()
        .map(|evidence| (evidence.item_id.as_str(), evidence.project.as_str()))
        .collect();
    assert_eq!(projects["alpha-item"], "alpha");
    assert_eq!(projects["zeta-item"], "zeta");
    assert_eq!(
        bundle.current.metrics.project_ranking,
        vec![("alpha".into(), 1), ("zeta".into(), 1)]
    );
}
