use super::*;
use std::collections::HashMap;

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
    assert_eq!(layer.indicators[1].name, "decision_quality");
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
    assert_eq!(layer.indicators[2].name, "bug_decision");
}
