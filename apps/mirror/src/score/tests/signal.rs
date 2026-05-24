use super::*;
use std::collections::HashMap;
use std::io::IsTerminal;

#[test]
fn test_signal_conversions_are_canonical() {
    let cases = [
        (Signal::Green, "green", "🟢", "\x1b[32m●\x1b[0m"),
        (Signal::Yellow, "yellow", "🟡", "\x1b[33m●\x1b[0m"),
        (Signal::Red, "red", "🔴", "\x1b[31m●\x1b[0m"),
    ];

    for (signal, plain, emoji, ansi) in cases {
        assert_eq!(signal.as_str(), plain);
        assert_eq!(signal.emoji(), emoji);
        assert_eq!(signal.ansi_dot(), ansi);
        assert_eq!(signal.plain_dot(), emoji);
        assert_eq!(signal.render(true), ansi);
        assert_eq!(signal.render(false), emoji);

        let expected_display = if std::io::stdout().is_terminal() {
            ansi
        } else {
            emoji
        };
        assert_eq!(signal.to_string(), expected_display);
    }
}

#[test]
fn test_dreyfus_weighted_calculation() {
    let mut cog = HashMap::new();
    cog.insert("expert".into(), 5);
    cog.insert("proficient".into(), 5);
    let stats = refine_core::session::GlobalStats {
        total_sessions: 10,
        total_decisions: 0,
        total_bugfixes: 0,
        total_summaries: 0,
        cognitive_levels: cog,
        collaboration_modes: HashMap::new(),
        tool_frequency: HashMap::new(),
        project_ranking: Vec::new(),
    };
    let dw = dreyfus_weighted(&stats);
    // (5*5 + 5*4) / 10 = 4.5
    assert!((dw - 4.5).abs() < f64::EPSILON);
}

#[test]
fn test_signal_from_thresholds() {
    let t = crate::config::Targets::default();
    let cluster = make_cluster(
        {
            let mut m = HashMap::new();
            m.insert("expert".into(), 10);
            m
        },
        {
            let mut m = HashMap::new();
            m.insert("exploration".into(), 20);
            m.insert("delegation".into(), 10);
            m.insert("pair_programming".into(), 10);
            m.insert("review".into(), 10);
            m.insert("deep_inquiry".into(), 10);
            m
        },
        50,
        10,
        vec![
            ("proj-a", 25, vec!["因为性能选择 Rust", "采用 SQLite"]),
            ("proj-b", 5, vec!["修复 bug"]),
        ],
    );
    let result = compute(&cluster, &t);
    // Dreyfus = 5.0 → green
    assert_eq!(result.layers[0].indicators[0].signal, Signal::Green);
}

#[test]
fn test_layer_signal_worst_of_three() {
    let t = crate::config::Targets::default();
    let cluster = make_cluster(
        {
            let mut m = HashMap::new();
            m.insert("novice".into(), 10); // dreyfus = 1.0 → red
            m
        },
        {
            let mut m = HashMap::new();
            m.insert("delegation".into(), 80);
            m.insert("exploration".into(), 20);
            m
        },
        10,
        2,
        vec![("proj-a", 5, vec!["选择 X 因为 Y"])],
    );
    let result = compute(&cluster, &t);
    // layer1 dreyfus=1.0 red, so layer1 = red
    assert_eq!(result.layers[0].signal, Signal::Red);
}

#[test]
fn test_tension_analysis() {
    let green_layer = LayerScore {
        name: "test".into(),
        signal: Signal::Green,
        indicators: Vec::new(),
    };
    let red_layer = LayerScore {
        name: "test".into(),
        signal: Signal::Red,
        indicators: Vec::new(),
    };
    // L1 green + L2 red
    let tension = analyze_tension(&[green_layer.clone(), red_layer.clone(), green_layer.clone()]);
    assert!(tension.is_some());
    assert!(tension.unwrap_or_default().contains("narrowing"));

    // All green
    let tension = analyze_tension(&[
        green_layer.clone(),
        green_layer.clone(),
        green_layer.clone(),
    ]);
    assert!(tension.unwrap_or_default().contains("healthy"));

    // All red
    let tension = analyze_tension(&[red_layer.clone(), red_layer.clone(), red_layer.clone()]);
    assert!(tension.unwrap_or_default().contains("replan"));
}
