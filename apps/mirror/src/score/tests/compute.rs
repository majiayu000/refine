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

    assert!((deep_invest.actual - (1.0 / 3.0 * 100.0)).abs() < 0.0001);
    assert!((fragmentation.actual - (2.0 / 3.0 * 100.0)).abs() < 0.0001);
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
