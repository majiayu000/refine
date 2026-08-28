#![allow(dead_code)]

#[path = "../src/cognitive_portrait_data/mod.rs"]
mod cognitive_portrait_data;
#[path = "../src/insights_manifest.rs"]
mod insights_manifest;

use chrono::{DateTime, Duration, TimeZone, Utc};
use cognitive_portrait_data::{
    build_bundle_from_snapshot, collect_bundle, read_bundle, validate_files, validate_portrait,
    write_bundle, CognitivePortraitBundle, MAX_PORTRAIT_BUNDLE_BYTES, MAX_PORTRAIT_CANDIDATE_BYTES,
    MAX_PREVIOUS_PORTRAIT_BYTES, MAX_WINDOW_DIMENSIONS_BYTES, PORTRAIT_BUNDLE_SCHEMA_VERSION,
    PORTRAIT_COLLECTOR_VERSION,
};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{
    Document, DocumentId, DocumentRepository, Item, ItemId, ItemType, ObservationDocumentMeta,
    ObservationWindowSnapshot, RestoreDocumentParams, RestoreParams, Source, Tag,
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

fn claim_line(bundle: &CognitivePortraitBundle, claim_id: &str) -> String {
    bundle
        .claim_catalog
        .claims
        .iter()
        .find(|claim| claim.claim_id == claim_id)
        .unwrap_or_else(|| panic!("missing claim {claim_id}"))
        .rendered_line
        .clone()
}

fn valid_action() -> &'static str {
    "[建议] 在截止日前检查 cohort。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|\"OK\"]"
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
    let claim_ids: Vec<_> = bundle
        .claim_catalog
        .claims
        .iter()
        .map(|claim| claim.claim_id.as_str())
        .collect();
    assert!(claim_ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(claim_ids.contains(&"fact.current.total_sessions"));
    assert!(claim_ids.contains(&"fact.current.evidence_selection.omitted_observations"));
    assert!(claim_ids.contains(&"fact.current.dimensions.knowledge.omitted_occurrences"));
    assert!(claim_ids.contains(&"fact.current.metrics.project_ranking.omitted_occurrences"));
    assert!(claim_ids.contains(&"fact.current.metrics.tool_frequency.selected_entries"));
    assert!(claim_ids.contains(&"fact.current.manifest.source.claude.coverage"));
    assert!(claim_ids.contains(&"fact.current.manifest.source.codex.coverage"));
    assert!(claim_ids.contains(&"fact.current.manifest.source.platform_unknown.coverage"));
    assert!(claim_ids.contains(&"fact.current.manifest.unsupported_sources.coverage"));
    assert!(claim_ids.contains(&"fact.current.evidence.000000"));
    assert!(claim_ids.contains(&"trend.total_sessions"));
}

#[tokio::test]
async fn sqlite_portrait_preserves_bounded_resolver_evidence() {
    let store = SqliteStore::in_memory().unwrap();
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let captured_at = cutoff - Duration::days(1);
    let oversized_project = format!("/r/{}a", "x".repeat(1_021));
    assert_eq!(oversized_project.len(), 1_025);
    let fixtures = [
        ("dot", "/r/a.b/foo"),
        ("hyphen", "/r/a-b/foo"),
        ("alias", "foo"),
        ("tool-path", "/users/me/work/my-tools-app"),
        ("independent-app", "app"),
        ("encoded-a", "-root-a-work-mutil-om"),
        ("encoded-b", "-root-b-work-mutil-om"),
        ("encoded-alias", "om"),
        ("oversized", oversized_project.as_str()),
    ];
    for (id, project) in fixtures {
        let document_id = DocumentId::from(format!("{id}-doc").as_str());
        let document = Document::restore(RestoreDocumentParams {
            id: document_id.clone(),
            title: Some(id.into()),
            raw_content: format!("raw {id}"),
            source: "codex-session".into(),
            url: format!("sqlite-fixture://{id}"),
            source_version: None,
            captured_at,
            created_at: captured_at,
            updated_at: captured_at,
        });
        let mut item = observation(
            id,
            Some(document_id.as_str()),
            captured_at,
            &[if project.len() > 50 {
                "oversized"
            } else {
                project
            }],
            "进展:\n- exact sqlite portrait fixture",
        );
        item.set_source(Source::new("session-project").with_url(project));
        DocumentRepository::save_with_replaced_items(&store, &document, &[item])
            .await
            .unwrap();
    }

    let bundle = collect_bundle(&store, cutoff, 90).await.unwrap();
    let projects: std::collections::BTreeMap<_, _> = bundle
        .current
        .evidence
        .iter()
        .map(|evidence| (evidence.item_id.as_str(), evidence.project.as_str()))
        .collect();
    assert_eq!(projects["dot"], "path:/r/a.b/foo");
    assert_eq!(projects["hyphen"], "path:/r/a-b/foo");
    assert_eq!(projects["alias"], "other");
    assert_eq!(projects["tool-path"], "my-tools-app");
    assert_eq!(projects["independent-app"], "other");
    assert_eq!(projects["encoded-a"], "encoded:-root-a-work-mutil-om");
    assert_eq!(projects["encoded-b"], "encoded:-root-b-work-mutil-om");
    assert_eq!(projects["encoded-alias"], "other");
    let oversized_identity = projects["oversized"];
    assert!(oversized_identity.starts_with("path:/r/"));
    assert!(oversized_identity.contains("~bytes=1025;sha256="));
    assert!(oversized_identity.len() <= 512);
    assert_eq!(
        bundle
            .manifest
            .current_window
            .ambiguous_project_alias_observations,
        3
    );
    assert_eq!(bundle.manifest.current_window.ambiguous_project_aliases, 3);
    let ranking: std::collections::BTreeMap<_, _> = bundle
        .current
        .metrics
        .project_ranking
        .entries
        .iter()
        .map(|entry| (entry.value.as_str(), entry.count))
        .collect();
    assert_eq!(ranking["path:/r/a.b/foo"], 1);
    assert_eq!(ranking["path:/r/a-b/foo"], 1);
    assert_eq!(ranking["my-tools-app"], 1);
    assert_eq!(ranking["encoded:-root-a-work-mutil-om"], 1);
    assert_eq!(ranking["encoded:-root-b-work-mutil-om"], 1);
    assert_eq!(ranking[oversized_identity], 1);
    assert_eq!(ranking["other"], 3);
    assert!(!ranking.contains_key("foo"));
    assert!(!ranking.contains_key("app"));
    assert!(!ranking.contains_key("om"));
}

#[test]
fn required_projection_disclosures_are_canonical_catalog_claims() {
    let bundle = fixture(false);
    let candidate = portrait(&format!(
        "{}\n\n{}",
        claim_line(
            &bundle,
            "fact.current.evidence_selection.omitted_observations"
        ),
        valid_action()
    ));
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(report.passed, "{:?}", report.errors);
    assert_eq!(report.unsupported_numeric_claims, 0);
}

#[test]
fn required_source_counts_and_freshness_are_canonical_catalog_claims() {
    let bundle = fixture(false);
    assert!(
        claim_line(&bundle, "fact.current.manifest.source.codex.coverage")
            .contains("freshest_event_time=2026-08-27T00:00:00+00:00")
    );
    assert!(
        claim_line(&bundle, "fact.current.manifest.source.claude.coverage").contains(
            "observations=0 observation; sessions=0 session; freshest_event_time=unavailable"
        )
    );
    let body = [
        "fact.current.manifest.source.claude.coverage",
        "fact.current.manifest.source.codex.coverage",
        "fact.current.manifest.source.platform_unknown.coverage",
        "fact.current.manifest.unsupported_sources.coverage",
    ]
    .map(|claim_id| claim_line(&bundle, claim_id))
    .join("\n\n");
    let candidate = portrait(&format!("{body}\n\n{}", valid_action()));
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(report.passed, "{:?}", report.errors);
    assert_eq!(report.factual_claims, 4);
    assert_eq!(report.unsupported_numeric_claims, 0);
}

#[test]
fn unsupported_knowledge_source_is_disclosed_and_suppresses_trends() {
    let bundle = fixture(true);
    assert_eq!(bundle.current.metrics.total_sessions, 1);
    assert_eq!(bundle.current.evidence.len(), 1);
    assert_eq!(
        bundle.manifest.current_window.unsupported_sources.entries[0].source,
        "grok-knowledge"
    );
    assert!(!bundle.comparison.comparable);
    assert_eq!(bundle.comparison.status, "DEGRADED");
    assert!(bundle
        .claim_catalog
        .claims
        .iter()
        .all(|claim| claim.kind != "trend"));
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
    assert_eq!(bundle.current.evidence_selection.eligible_observations, 1);
    assert_eq!(bundle.current.evidence_selection.selected_observations, 1);
    assert_eq!(bundle.current.evidence_selection.omitted_observations, 0);
    assert!(bundle
        .current
        .evidence_selection
        .full_payload_digest
        .starts_with("sha256:"));
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

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["claim_catalog"]["claims"][0]["rendered_line"] = serde_json::json!("spoofed");
    std::fs::write(&invalid, serde_json::to_vec(&tampered).unwrap()).unwrap();
    assert!(read_bundle(&invalid)
        .unwrap_err()
        .to_string()
        .contains("claim catalog"));
}

#[test]
fn full_payload_digest_distinguishes_omitted_null_and_empty_excerpt() {
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let event_time = cutoff - Duration::days(1);
    let make_item = |id: &str, excerpt: Option<String>| {
        Item::restore(RestoreParams {
            id: ItemId::from(id),
            item_type: ItemType::Observation,
            title: id.to_string(),
            summary: String::new(),
            content: String::new(),
            tags: vec![Tag::new("refine").unwrap()],
            source: None,
            document_id: Some(DocumentId::from("digest-doc")),
            excerpt,
            created_at: event_time,
            updated_at: event_time,
        })
        .unwrap()
    };
    let build = |excerpt: Option<String>| {
        let mut current: Vec<Item> = (0..2049)
            .map(|index| make_item(&format!("item-{index:04}"), Some("same".to_string())))
            .collect();
        current.push(make_item("zzzz-omitted", excerpt));
        build_bundle_from_snapshot(
            ObservationWindowSnapshot {
                current,
                previous: Vec::new(),
                documents: vec![ObservationDocumentMeta {
                    id: DocumentId::from("digest-doc"),
                    source: "codex-session".to_string(),
                    captured_at: event_time,
                }],
            },
            cutoff,
            90,
        )
        .unwrap()
    };
    let null_excerpt = build(None);
    let empty_excerpt = build(Some(String::new()));
    assert!(!null_excerpt
        .current
        .evidence
        .iter()
        .any(|record| record.item_id == "zzzz-omitted"));
    assert_ne!(
        null_excerpt.current.evidence_selection.full_payload_digest,
        empty_excerpt.current.evidence_selection.full_payload_digest
    );
}

#[test]
fn dimension_projection_packs_final_escaped_json_bytes() {
    let cutoff = Utc.with_ymd_and_hms(2026, 8, 28, 0, 0, 0).unwrap();
    let event_time = cutoff - Duration::days(1);
    let escaped = "\u{1}".repeat(500);
    let current: Vec<Item> = (0..256)
        .map(|index| {
            let category = if index < 128 { "decision" } else { "bugfix" };
            observation_with_title(
                &format!("escaped-dimension-{index:03}"),
                &format!("{escaped}-title-{index:03}"),
                Some("escaped-dimension-current"),
                event_time,
                &["refine", category],
                &format!(
                    "知识:\n- {escaped}-knowledge-{index:03}\n模式:\n- {escaped}-pattern-{index:03}\n架构:\n- {escaped}-architecture-{index:03}\n阻力:\n- {escaped}-friction-{index:03}"
                ),
            )
        })
        .collect();
    let snapshot = || ObservationWindowSnapshot {
        current: current.clone(),
        previous: vec![observation(
            "escaped-dimension-previous",
            Some("escaped-dimension-previous-doc"),
            cutoff - Duration::days(91),
            &["refine"],
            "知识:\n- previous",
        )],
        documents: vec![
            ObservationDocumentMeta {
                id: DocumentId::from("escaped-dimension-current"),
                source: "codex-session".to_string(),
                captured_at: event_time,
            },
            ObservationDocumentMeta {
                id: DocumentId::from("escaped-dimension-previous-doc"),
                source: "claude-code-session".to_string(),
                captured_at: cutoff - Duration::days(91),
            },
        ],
    };
    let first = build_bundle_from_snapshot(snapshot(), cutoff, 90).unwrap();
    let second = build_bundle_from_snapshot(snapshot(), cutoff, 90).unwrap();
    let bytes = serde_json::to_vec(&first.current.dimensions).unwrap().len();
    assert!(
        bytes <= MAX_WINDOW_DIMENSIONS_BYTES,
        "dimensions use {bytes} bytes"
    );
    assert!(
        first.current.dimensions.knowledge.selected_values < 128
            || first.current.dimensions.patterns.selected_values < 128
            || first.current.dimensions.architectures.selected_values < 128
            || first.current.dimensions.frictions.selected_values < 128
    );
    for dimension in [
        &first.current.dimensions.knowledge,
        &first.current.dimensions.patterns,
        &first.current.dimensions.architectures,
        &first.current.dimensions.frictions,
    ] {
        assert_eq!(dimension.total_occurrences, 256);
        assert_eq!(
            dimension.selected_occurrences + dimension.omitted_occurrences,
            dimension.total_occurrences
        );
        assert!(dimension.full_digest.starts_with("sha256:"));
    }
    assert_eq!(first.current.dimensions, second.current.dimensions);
}

#[test]
fn bounded_projection_invariants_fail_closed() {
    let bundle = fixture(false);
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid-projection.json");
    let assert_invalid = |value: serde_json::Value, expected: &str| {
        std::fs::write(&invalid, serde_json::to_vec(&value).unwrap()).unwrap();
        let error = read_bundle(&invalid).unwrap_err().to_string();
        assert!(error.contains(expected), "unexpected error: {error}");
    };

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["current"]["evidence_selection"]["omitted_observations"] = serde_json::json!(1);
    assert_invalid(tampered, "evidence selection invariant");

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["current"]["dimensions"]["projects"]["entries"][0]["evidence_ids"][0] =
        serde_json::json!("obs:does-not-exist");
    assert_invalid(tampered, "dangling evidence reference");

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["current"]["evidence_selection"]["full_payload_digest"] =
        serde_json::json!("sha256:not-a-digest");
    assert_invalid(tampered, "evidence selection invariant");

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["manifest"]["manifest_version"] = serde_json::json!(1);
    assert_invalid(tampered, "unsupported insights manifest schema 1");

    let mut tampered = serde_json::to_value(&bundle).unwrap();
    tampered["current"]["metrics"]["project_ranking"]["omitted_occurrences"] =
        serde_json::json!(999);
    assert_invalid(tampered, "count breakdown invariant");

    let mut tampered = serde_json::to_value(fixture(true)).unwrap();
    tampered["manifest"]["current_window"]["unsupported_sources"]["selected_observations"] =
        serde_json::json!(999);
    assert_invalid(tampered, "unsupported source breakdown invariant");
}

#[test]
fn quality_gate_requires_evidence_numbers_novelty_and_verifiable_actions() {
    let bundle = fixture(false);
    let candidate = portrait(&format!(
        "{}\n\n{}\n\n这是一段足够长且全新的认知分析内容，用于证明本期具备真实新增信息。",
        claim_line(&bundle, "fact.current.total_sessions"),
        valid_action()
    ));
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
fn degraded_comparison_disables_portrait_validation_entirely() {
    let bundle = fixture(true);
    let candidate = portrait(&format!(
        "{}\n\n{}",
        claim_line(&bundle, "fact.current.total_sessions"),
        valid_action()
    ));
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(!report.passed);
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("generation is disabled")));
}

#[test]
fn comparable_trends_require_explicit_marker_and_both_window_scalars() {
    let bundle = fixture(false);
    let valid = portrait(&format!(
        "{}\n\n{}",
        claim_line(&bundle, "trend.total_sessions"),
        valid_action()
    ));
    let report = validate_portrait(&bundle, &valid, None);
    assert!(report.passed, "{:?}", report.errors);

    let implicit = portrait(&format!(
        "[事实] session 总量较上期持平。[bundle:/current/metrics/total_sessions] [bundle:/previous/metrics/total_sessions]\n\n{}",
        valid_action()
    ));
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
        "[事实] 当前窗口有 [999] 个 session。[bundle:/schema_version]",
        "[事实] 当前窗口 session 总量见指标。[claim:unknown]",
        "[事实] 当前窗口 session 总量见指标。[claim:fact.current.total_sessions]",
    ] {
        let candidate = portrait(&format!("{claim}\n\n{}", valid_action()));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "claim unexpectedly passed: {claim}");
    }

    let spoof = portrait(&format!(
        "[事实][claim:fact.current.total_sessions] 当前决策总量：1 decision。\n\n{}",
        valid_action()
    ));
    assert!(!validate_portrait(&bundle, &spoof, None).passed);

    let canonical = claim_line(&bundle, "fact.current.total_sessions");
    let duplicate = portrait(&format!("{canonical}\n\n{canonical}\n\n{}", valid_action()));
    assert!(!validate_portrait(&bundle, &duplicate, None).passed);
}

#[test]
fn non_numeric_facts_must_use_the_closed_evidence_catalog() {
    let bundle = fixture(false);
    let evidence_fact = claim_line(&bundle, "fact.current.evidence.000000");
    let valid = portrait(&format!("{evidence_fact}\n\n{}", valid_action()));
    assert!(validate_portrait(&bundle, &valid, None).passed);

    let invented = portrait(&format!(
        "[事实] 当前工作明显更成熟。[evidence:obs:current]\n\n{}",
        valid_action()
    ));
    let report = validate_portrait(&bundle, &invented, None);
    assert!(!report.passed);
    assert_eq!(report.factual_traceability_rate, 0.0);
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
    let previous = portrait(&format!("{}\n\n{}\n\n[可见标签](https://old.example) 这一段分析文字在两期完全相同，只修改隐藏元数据也不能算作新增洞察。\n\n[ref]: https://old.example", claim_line(&bundle, "fact.current.total_sessions"), valid_action()));
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
    let hidden = portrait(&format!(
        "{}\n\n{}",
        claim_line(&bundle, "fact.current.total_sessions"),
        valid_action()
    ));
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

    let metadata_verify = portrait("[事实] 当前窗口状态已采集。[bundle:/comparison/status]\n\n[建议] 核对统计。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/schema_version|eq|1]");
    let report = validate_portrait(&bundle, &metadata_verify, None);
    assert!(!report.passed);
    assert_eq!(report.verifiable_actions, 0);

    let typed_mismatch = portrait("[事实] 当前窗口状态已采集。[bundle:/comparison/status]\n\n[建议] 核对统计。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/current/metrics/total_sessions|eq|\"banana\"]");
    assert_eq!(
        validate_portrait(&bundle, &typed_mismatch, None).verifiable_actions,
        0
    );

    let noncanonical_target = portrait("[事实] 当前窗口状态已采集。[bundle:/comparison/status]\n\n[建议] 核对统计。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/current/metrics/total_sessions|eq|1e0]");
    assert_eq!(
        validate_portrait(&bundle, &noncanonical_target, None).verifiable_actions,
        0
    );
}

#[test]
fn soft_wrapping_visible_prose_does_not_create_false_novelty() {
    let bundle = fixture(false);
    let prose = "同一段可见分析文字即使只改变 Markdown 源码软换行，也不能被统计为新的认知洞察。";
    let previous = portrait(&format!(
        "{}\n\n{}\n\n{prose}",
        claim_line(&bundle, "fact.current.total_sessions"),
        valid_action()
    ));
    let candidate = previous.replace("即使只改变", "即使\n只改变");
    let report = validate_portrait(&bundle, &candidate, Some(&previous));
    assert!(!report.passed);
    assert_eq!(report.novelty_rate, Some(0.0));
}

#[test]
fn catalog_claims_in_quotes_code_or_html_do_not_count() {
    let bundle = fixture(false);
    let claim = claim_line(&bundle, "fact.current.total_sessions");
    for hidden in [format!("> {claim}"), format!("<!-- {claim} -->")] {
        let candidate = portrait(&format!("{hidden}\n\n{}", valid_action()));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "hidden claim unexpectedly passed: {hidden}");
        assert_eq!(report.factual_claims, 0);
    }

    let inline_code = portrait(&format!("`{claim}`\n\n{}", valid_action()));
    let report = validate_portrait(&bundle, &inline_code, None);
    assert!(!report.passed);
    assert_eq!(report.factual_claims, 1);
    assert_eq!(report.unsupported_numeric_claims, 1);

    let html = portrait(&format!("<span>{claim}</span>\n\n{}", valid_action()));
    let report = validate_portrait(&bundle, &html, None);
    assert!(!report.passed);
    assert!(report.errors.iter().any(|error| error.contains("raw HTML")));

    let reformatted = portrait(&format!("**{claim}**\n\n{}", valid_action()));
    let report = validate_portrait(&bundle, &reformatted, None);
    assert!(!report.passed);
    assert_eq!(report.factual_claims, 1);
    assert_eq!(report.unsupported_numeric_claims, 1);
}

#[test]
fn inferences_require_valid_allowlisted_evidence() {
    let bundle = fixture(false);
    let fact = claim_line(&bundle, "fact.current.total_sessions");
    for inference in [
        "[推断，置信度：高] 当前工作明显更成熟。",
        "[推断，置信度：高] 当前工作明显更成熟。[evidence:obs:does-not-exist]",
        "[推断，置信度：高] 当前工作明显更成熟。[bundle:/schema_version]",
        "[推断，置信度：高] 当前工作明显更成熟。[evidence:obs:current] [bundle:/schema_version]",
    ] {
        let candidate = portrait(&format!("{fact}\n\n{inference}\n\n{}", valid_action()));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "inference unexpectedly passed: {inference}");
        assert_eq!(report.inference_traceability_rate, 0.0);
    }

    let valid = portrait(&format!(
        "{fact}\n\n[推断，置信度：高] 当前工作形成可复核证据。[evidence:obs:current]\n\n{}",
        valid_action()
    ));
    let report = validate_portrait(&bundle, &valid, None);
    assert!(report.passed, "{:?}", report.errors);
    assert_eq!(report.inference_traceability_rate, 1.0);
}

#[test]
fn inline_code_is_visible_to_claim_and_novelty_gates() {
    let bundle = fixture(false);
    let fact = claim_line(&bundle, "fact.current.total_sessions");
    let inference = portrait(&format!(
        "{fact}\n\n[推断，置信度：高] session 数为 `999`。[evidence:obs:current]\n\n{}",
        valid_action()
    ));
    let report = validate_portrait(&bundle, &inference, None);
    assert!(!report.passed);
    assert_eq!(report.inference_traceability_rate, 1.0);
    assert!(report.unsupported_numeric_claims > 0);

    let inline_fact = portrait(&format!(
        "[事实] 当前窗口有 `999` 个 session。[evidence:obs:current]\n\n{}",
        valid_action()
    ));
    let report = validate_portrait(&bundle, &inline_fact, None);
    assert!(!report.passed);
    assert!(report.unsupported_numeric_claims > 0);

    let inline_action = portrait(&format!(
        "{fact}\n\n[建议] 执行 `999` 次检查。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|\"OK\"]"
    ));
    let report = validate_portrait(&bundle, &inline_action, None);
    assert!(!report.passed);
    assert!(report.unsupported_numeric_claims > 0);

    for unsupported_body in [
        format!(
            "[推断，置信度：高] 当前完成 `٩٩٩` 个任务。[evidence:obs:current]\n\n{}",
            valid_action()
        ),
        format!(
            "[推断，置信度：高] 当前完成 ⑨⑨⑨ 个任务。[evidence:obs:current]\n\n{}",
            valid_action()
        ),
        "[建议] 执行 `۹۹۹` 次检查。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|\"OK\"]".to_string(),
        "[建议] 执行 ९९९ 次检查。[evidence:obs:current] [owner:lifcc] [due:2026-09-01] [verify:metric|/comparison/status|eq|\"OK\"]".to_string(),
    ] {
        let candidate = portrait(&format!("{fact}\n\n{unsupported_body}"));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "Unicode numeric claim unexpectedly passed");
        assert!(report.unsupported_numeric_claims > 0);
    }

    let previous = portrait(&format!(
        "{fact}\n\n{}\n\n这是一段足够长的重复分析，可见编号 999 不会因为 Markdown 行内代码格式变化而变成新洞察。",
        valid_action()
    ));
    let candidate = previous.replace("可见编号 999", "可见编号 `999`");
    let report = validate_portrait(&bundle, &candidate, Some(&previous));
    assert!(!report.passed);
    assert_eq!(report.novelty_rate, Some(0.0));
}

#[test]
fn active_markdown_payloads_fail_closed_but_safe_links_pass() {
    let bundle = fixture(false);
    let fact = claim_line(&bundle, "fact.current.total_sessions");
    for payload in [
        "<script>alert('x')</script>",
        "![tracking](https://example.com/pixel.png)",
        "[payload](javascript:alert(1))",
        "[payload](data:text/html,boom)",
        "[payload](file:///etc/passwd)",
        "[payload](/absolute/path)",
        "[payload](../outside)",
    ] {
        let candidate = portrait(&format!("{fact}\n\n{}\n\n{payload}", valid_action()));
        let report = validate_portrait(&bundle, &candidate, None);
        assert!(!report.passed, "payload unexpectedly passed: {payload}");
        assert!(report.errors.iter().any(|error| {
            error.contains("raw HTML")
                || error.contains("images")
                || error.contains("unsafe Markdown link")
        }));
    }

    let safe = portrait(&format!(
        "{fact}\n\n{}\n\n[说明](https://example.com/report) 与 [本地规范](./SPEC.md)。",
        valid_action()
    ));
    let report = validate_portrait(&bundle, &safe, None);
    assert!(report.passed, "{:?}", report.errors);
}

#[test]
fn candidate_line_block_and_file_sizes_are_bounded() {
    let bundle = fixture(false);
    let fact = claim_line(&bundle, "fact.current.total_sessions");
    let long_line = "x".repeat(64 * 1024 + 1);
    let candidate = portrait(&format!("{fact}\n\n{}\n\n{long_line}", valid_action()));
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(!report.passed);
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("line exceeds")));

    let many_blocks = (0..4100)
        .map(|index| format!("block-{index}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let candidate = portrait(&format!("{fact}\n\n{}\n\n{many_blocks}", valid_action()));
    let report = validate_portrait(&bundle, &candidate, None);
    assert!(!report.passed);
    assert!(report
        .errors
        .iter()
        .any(|error| error.contains("block limit")));

    let directory = tempfile::tempdir().unwrap();
    let bundle_path = directory.path().join("bundle.json");
    let candidate_path = directory.path().join("candidate.md");
    let previous_path = directory.path().join("previous.md");
    let quality_path = directory.path().join("quality.json");
    write_bundle(&bundle_path, &bundle).unwrap();
    std::fs::File::create(&candidate_path)
        .unwrap()
        .set_len((MAX_PORTRAIT_CANDIDATE_BYTES + 1) as u64)
        .unwrap();
    assert!(
        validate_files(&bundle_path, &candidate_path, None, &quality_path)
            .unwrap_err()
            .to_string()
            .contains("byte limit")
    );

    std::fs::write(
        &candidate_path,
        portrait(&format!("{fact}\n\n{}", valid_action())),
    )
    .unwrap();
    std::fs::File::create(&previous_path)
        .unwrap()
        .set_len((MAX_PREVIOUS_PORTRAIT_BYTES + 1) as u64)
        .unwrap();
    assert!(validate_files(
        &bundle_path,
        &candidate_path,
        Some(&previous_path),
        &quality_path
    )
    .unwrap_err()
    .to_string()
    .contains("byte limit"));

    let oversized_bundle = directory.path().join("oversized-bundle.json");
    std::fs::File::create(&oversized_bundle)
        .unwrap()
        .set_len((MAX_PORTRAIT_BUNDLE_BYTES + 1) as u64)
        .unwrap();
    assert!(read_bundle(&oversized_bundle)
        .unwrap_err()
        .to_string()
        .contains("byte limit"));
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
        bundle
            .current
            .metrics
            .project_ranking
            .entries
            .iter()
            .map(|entry| (entry.value.as_str(), entry.count))
            .collect::<Vec<_>>(),
        vec![("alpha", 1), ("zeta", 1)]
    );
}
