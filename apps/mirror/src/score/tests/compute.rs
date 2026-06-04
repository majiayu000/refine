use super::*;
use std::collections::HashMap;

#[test]
fn test_knowledge_rate_green() {
    let t = crate::config::Targets::default();
    // 10 sessions, 6 knowledge items -> rate = 0.6 >= 0.5 -> green
    let cluster = make_cluster_with_data(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec![],
                vec!["learned Rust", "learned SQL", "learned async"],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec![],
                vec!["learned Docker", "learned K8s", "learned Nix"],
            ),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.6).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1
        .indicators
        .iter()
        .find(|i| i.name == "knowledge_rate")
        .unwrap();
    assert_eq!(kr_ind.signal, Signal::Green);
}

#[test]
fn test_knowledge_rate_red() {
    let t = crate::config::Targets::default();
    // 10 sessions, 1 knowledge item -> rate = 0.1 < 0.2 -> red
    let cluster = make_cluster_with_data(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![
            ("proj-a", 5, vec![], vec![], vec!["learned Rust"]),
            ("proj-b", 5, vec![], vec![], vec![]),
        ],
    );
    let kr = knowledge_rate(&cluster);
    assert!((kr - 0.1).abs() < f64::EPSILON);
    let l1 = layer1(&cluster, &t);
    let kr_ind = l1
        .indicators
        .iter()
        .find(|i| i.name == "knowledge_rate")
        .unwrap();
    assert_eq!(kr_ind.signal, Signal::Red);
}

#[test]
fn test_friction_density_green() {
    let t = crate::config::Targets::default();
    // 10 sessions, 5 frictions -> density = 0.5 < 1.0 -> green
    let cluster = make_cluster_with_data(
        HashMap::new(),
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 5);
            m.insert("exploration".into(), 5);
            m.insert("pair_programming".into(), 5);
            m.insert("review".into(), 5);
            m
        },
        10,
        2,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec!["slow build", "confusing API"],
                vec![],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec!["flaky test", "bad docs", "timeout"],
                vec![],
            ),
        ],
    );
    let fd = friction_density(&cluster);
    assert!((fd - 0.5).abs() < f64::EPSILON);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3
        .indicators
        .iter()
        .find(|i| i.name == "friction_density")
        .unwrap();
    assert_eq!(fd_ind.signal, Signal::Green);
}

#[test]
fn test_friction_density_red() {
    let t = crate::config::Targets::default();
    // 10 sessions, 25 frictions -> density = 2.5 > 2.0 -> red
    let cluster = make_cluster_with_data(
        HashMap::new(),
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 5);
            m.insert("exploration".into(), 5);
            m.insert("pair_programming".into(), 5);
            m.insert("review".into(), 5);
            m
        },
        10,
        2,
        vec![
            (
                "proj-a",
                5,
                vec![],
                vec![
                    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
                    "f13",
                ],
                vec![],
            ),
            (
                "proj-b",
                5,
                vec![],
                vec![
                    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
                ],
                vec![],
            ),
        ],
    );
    let fd = friction_density(&cluster);
    assert!(fd > 2.0);
    let l3 = layer3(&cluster, &t);
    let fd_ind = l3
        .indicators
        .iter()
        .find(|i| i.name == "friction_density")
        .unwrap();
    assert_eq!(fd_ind.signal, Signal::Red);
}

#[test]
fn test_layer1_has_4_indicators() {
    let t = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let l1 = layer1(&cluster, &t);
    assert_eq!(l1.indicators.len(), 4);
    assert_eq!(l1.indicators[3].name, "knowledge_rate");
}

#[test]
fn test_layer3_has_4_indicators() {
    let t = crate::config::Targets::default();
    let cluster = make_cluster(
        HashMap::new(),
        HashMap::new(),
        0,
        0,
        vec![("proj-a", 5, vec![])],
    );
    let l3 = layer3(&cluster, &t);
    assert_eq!(l3.indicators.len(), 4);
    assert_eq!(l3.indicators[3].name, "friction_density");
}
