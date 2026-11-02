use super::*;
use std::collections::HashMap;

fn indicator<'a>(layer: &'a LayerScore, name: &str) -> &'a Indicator {
    layer
        .indicators
        .iter()
        .find(|indicator| indicator.name == name)
        .expect("indicator should exist")
}

#[test]
fn unknown_and_curation_metadata_do_not_fabricate_project_coverage() {
    use refine_core::knowledge::{DocumentId, Item, Tag};
    let mut item = Item::new_observation("No classified evidence", "Synthetic summary");
    item.set_document_id(DocumentId::from("synthetic-session"));
    item.set_tags(
        ["unknown", "curated"]
            .into_iter()
            .map(|tag| Tag::new(tag).unwrap())
            .collect(),
    )
    .unwrap();
    let cluster = refine_core::session::cluster_observations(&[item]);
    let result = compute(&cluster, &crate::config::Targets::default());
    for metric in &result.layers[1].indicators {
        assert_eq!(metric.signal, Signal::Unknown, "{}", metric.name);
        assert_eq!(metric.actual, None, "{}", metric.name);
    }
    let coverage = indicator(&result.layers[1], "fragmentation")
        .coverage
        .as_ref()
        .unwrap();
    assert_eq!(coverage.observed, 0);
    assert_eq!(coverage.eligible, 1);
}

#[test]
fn test_layer1_has_2_indicators() {
    let targets = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let layer = layer1(&cluster, &targets);
    assert_eq!(layer.indicators.len(), 2);
    assert_eq!(layer.indicators[0].name, "dreyfus");
    assert_eq!(layer.indicators[1].name, "decision_quality");
}

#[test]
fn test_layer2_uses_project_bucket_rates() {
    let targets = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            ("solo-a", 1, vec![]),
            ("solo-b", 1, vec![]),
            ("deep-a", 20, vec![]),
        ],
    );

    let score = compute(&cluster, &targets);
    let breadth = &score.layers[1];
    let deep_invest = indicator(breadth, "deep_invest");
    let fragmentation = indicator(breadth, "fragmentation");

    assert!((deep_invest.actual.unwrap() - (1.0 / 3.0 * 100.0)).abs() < 0.0001);
    assert!((fragmentation.actual.unwrap() - (2.0 / 3.0 * 100.0)).abs() < 0.0001);
}

#[test]
fn test_layer2_excludes_other_and_zero_session_projects() {
    let targets = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            ("other", 100, vec![]),
            ("empty", 0, vec![]),
            ("deep-a", 20, vec![]),
            ("solo-a", 1, vec![]),
        ],
    );

    let score = compute(&cluster, &targets);
    let breadth = &score.layers[1];
    assert_eq!(indicator(breadth, "deep_invest").actual, Some(50.0));
    assert_eq!(indicator(breadth, "fragmentation").actual, Some(50.0));
}

#[test]
fn test_layer2_empty_project_denominator_is_safe() {
    let targets = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("other", 5, vec![]), ("empty", 0, vec![])],
    );

    let score = compute(&cluster, &targets);
    let breadth = &score.layers[1];
    assert_eq!(indicator(breadth, "deep_invest").actual, None);
    assert_eq!(indicator(breadth, "fragmentation").actual, None);
}

#[test]
fn test_layer3_has_3_indicators() {
    let targets = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let layer = layer3(&cluster, &targets);
    assert_eq!(layer.indicators.len(), 3);
    assert_eq!(layer.indicators[0].name, "delegation");
    assert_eq!(layer.indicators[1].name, "mode_diversity");
    assert_eq!(layer.indicators[2].name, "bug_decision");
}

#[test]
fn missing_denominators_are_unknown_and_serialize_as_null() {
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        7,
        vec![("other", 2, vec![])],
    );
    let result = compute(&cluster, &crate::config::Targets::default());
    for layer in &result.layers {
        assert_eq!(layer.signal, Signal::Unknown);
        for indicator in &layer.indicators {
            assert_eq!(indicator.actual, None, "{}", indicator.name);
            assert_eq!(indicator.signal, Signal::Unknown);
            assert!(indicator.coverage.is_some());
            let json = serde_json::to_value(indicator).unwrap();
            assert!(json["actual"].is_null());
            assert!(!indicator.display_value().contains("0%"));
        }
    }
}

#[test]
fn bugfix_only_evidence_cannot_make_collaboration_green() {
    let modes = ["review", "exploration", "teaching", "pair_programming"]
        .into_iter()
        .map(|mode| (mode.to_string(), 1))
        .collect();
    let cluster = make_cluster(HashMap::new(), modes, 0, 7, vec![("project", 4, vec![])]);
    let collaboration = layer3(&cluster, &crate::config::Targets::default());
    assert_eq!(indicator(&collaboration, "delegation").actual, Some(0.0));
    assert_eq!(
        indicator(&collaboration, "mode_diversity").actual,
        Some(4.0)
    );
    assert_eq!(indicator(&collaboration, "bug_decision").actual, None);
    assert_eq!(collaboration.signal, Signal::Unknown);
}

#[test]
fn observed_zero_bugfixes_remains_a_measured_zero() {
    let cluster = make_cluster(HashMap::new(), HashMap::new(), 3, 0, vec![]);
    let collaboration = layer3(&cluster, &crate::config::Targets::default());
    let bugs = indicator(&collaboration, "bug_decision");
    assert_eq!(bugs.actual, Some(0.0));
    // Measured (not missing evidence) but deliberately unrated.
    assert_eq!(bugs.signal, Signal::Info);
    assert_eq!(bugs.observed_value(), Some(0.0));
    assert_eq!(bugs.coverage.as_ref().unwrap().observed, 3);
}

#[test]
fn partial_label_coverage_is_visible_without_fabricating_missing_labels() {
    let mut cluster = make_cluster(
        HashMap::from([("expert".to_string(), 1)]),
        HashMap::new(),
        0,
        0,
        vec![],
    );
    cluster.global_stats.total_summaries = 8;
    let depth = layer1(&cluster, &crate::config::Targets::default());
    let dreyfus = indicator(&depth, "dreyfus");
    assert_eq!(dreyfus.actual, Some(5.0));
    let coverage = dreyfus.coverage.as_ref().unwrap();
    assert_eq!((coverage.observed, coverage.eligible), (1, 8));
    assert!(dreyfus.coverage_label().contains("1/8"));
}

#[test]
fn choice_verbs_do_not_count_as_reasons_and_english_case_is_normalized() {
    for (titles, expected) in [
        (vec!["选择 Redis", "采用 SQLite", "selected Rust"], 0.0),
        (
            vec!["Because SQLite is embedded", "because SQLite is embedded"],
            100.0,
        ),
    ] {
        let cluster = make_cluster(
            HashMap::new(),
            HashMap::new(),
            titles.len(),
            0,
            vec![("project", 1, titles)],
        );
        let depth = layer1(&cluster, &crate::config::Targets::default());
        assert_eq!(indicator(&depth, "decision_quality").actual, Some(expected));
    }
}

#[test]
fn delegation_share_is_shown_but_not_rated() {
    for delegated in [0, 5, 10] {
        let modes = [("delegation", delegated), ("review", 10 - delegated)]
            .into_iter()
            .map(|(mode, count)| (mode.to_string(), count))
            .collect();
        let cluster = make_cluster(HashMap::new(), modes, 0, 7, vec![("project", 4, vec![])]);
        let collaboration = layer3(&cluster, &crate::config::Targets::default());
        let delegation = indicator(&collaboration, "delegation");
        assert_eq!(delegation.signal, Signal::Info, "delegated={delegated}");
        assert_eq!(delegation.actual, Some(delegated as f64 * 10.0));
        assert!(delegation.display_value().ends_with('%'));
    }
}

#[test]
fn full_delegation_does_not_turn_collaboration_red() {
    let modes = [
        ("delegation", 10),
        ("review", 0),
        ("exploration", 0),
        ("pair_programming", 0),
    ]
    .into_iter()
    .map(|(mode, count)| (mode.to_string(), count))
    .collect();
    let targets = crate::config::Targets {
        mode_diversity_green: 1,
        mode_diversity_yellow: 1,
        ..Default::default()
    };
    let cluster = make_cluster(HashMap::new(), modes, 10, 1, vec![("project", 4, vec![])]);
    let collaboration = layer3(&cluster, &targets);
    assert_eq!(indicator(&collaboration, "delegation").actual, Some(100.0));
    assert_eq!(collaboration.signal, Signal::Green);
}

#[test]
fn worst_ignores_unrated_indicators() {
    use super::super::types::worst;
    assert_eq!(worst(&[Signal::Info, Signal::Green]), Signal::Green);
    assert_eq!(worst(&[Signal::Info, Signal::Yellow]), Signal::Yellow);
    assert_eq!(worst(&[Signal::Info]), Signal::Unknown);
}

#[test]
fn high_bug_ratio_does_not_turn_collaboration_red() {
    let modes = [("review", 5), ("exploration", 5)]
        .into_iter()
        .map(|(mode, count)| (mode.to_string(), count))
        .collect();
    let targets = crate::config::Targets {
        mode_diversity_green: 2,
        mode_diversity_yellow: 1,
        ..Default::default()
    };
    // 9 bugfixes per decision would have been red under the old <0.6 target.
    let cluster = make_cluster(HashMap::new(), modes, 1, 9, vec![("project", 4, vec![])]);
    let collaboration = layer3(&cluster, &targets);
    assert_eq!(indicator(&collaboration, "bug_decision").actual, Some(9.0));
    assert_eq!(
        indicator(&collaboration, "bug_decision").signal,
        Signal::Info
    );
    assert_eq!(collaboration.signal, Signal::Green);
}
