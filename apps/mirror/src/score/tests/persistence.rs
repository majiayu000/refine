use super::*;
use chrono::{Duration, Utc};
use std::collections::HashSet;
use std::sync::{Arc, Barrier};

use super::super::persistence::{
    load_score_activity_from_path, persist_score_to_path, SCORE_SCHEMA_VERSION,
};

#[test]
fn test_persist_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let result = ScoreResult {
        layers: [
            LayerScore {
                name: "L1".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L2".into(),
                signal: Signal::Yellow,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L3".into(),
                signal: Signal::Red,
                indicators: Vec::new(),
            },
        ],
        tension: Some("test tension".into()),
        timestamp: Utc::now(),
    };

    persist_score_to_path(&path, &result).unwrap();

    let persisted = std::fs::read_to_string(&path).unwrap();
    let persisted: serde_json::Value = serde_json::from_str(persisted.trim()).unwrap();
    assert_eq!(
        persisted["score_schema_version"].as_u64(),
        Some(u64::from(SCORE_SCHEMA_VERSION))
    );

    let loaded = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].layers[0].signal, Signal::Green);
    assert_eq!(loaded[0].tension.as_deref(), Some("test tension"));
}

#[test]
fn test_persist_score_rotates_to_latest_365_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    for idx in 0..370 {
        let mut result = make_score_result(
            3.5,
            60.0,
            10.0,
            0.5,
            20.0,
            25.0,
            15.0,
            30.0,
            4.0,
            0.2,
            0.8,
            Utc::now() + Duration::seconds(idx),
        );
        result.tension = Some(format!("entry-{}", idx));
        persist_score_to_path(&path, &result).unwrap();
    }

    let loaded = load_recent_scores_from_path(&path, 400).unwrap();
    assert_eq!(loaded.len(), 365);
    assert_eq!(loaded[0].tension.as_deref(), Some("entry-5"));
    assert_eq!(
        loaded.last().and_then(|s| s.tension.as_deref()),
        Some("entry-369")
    );
}

#[test]
fn test_persist_score_concurrent_writes_preserve_all_new_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    for idx in 0..365usize {
        let mut seed = make_score_result(
            3.5,
            60.0,
            10.0,
            0.5,
            20.0,
            25.0,
            15.0,
            30.0,
            4.0,
            0.2,
            0.8,
            Utc::now() - Duration::days(100) + Duration::seconds(idx as i64),
        );
        seed.tension = Some(format!("seed-{}", idx));
        persist_score_to_path(&path, &seed).unwrap();
    }

    let writer_count = 12usize;
    let barrier = Arc::new(Barrier::new(writer_count));
    let mut handles = Vec::new();

    for idx in 0..writer_count {
        let barrier = Arc::clone(&barrier);
        let path = path.clone();
        handles.push(std::thread::spawn(move || {
            let mut result = make_score_result(
                3.5,
                60.0,
                10.0,
                0.5,
                20.0,
                25.0,
                15.0,
                30.0,
                4.0,
                0.2,
                0.8,
                Utc::now() + Duration::seconds(idx as i64),
            );
            result.tension = Some(format!("writer-{}", idx));
            barrier.wait();
            persist_score_to_path(&path, &result).unwrap();
        }));
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let loaded = load_recent_scores_from_path(&path, 400).unwrap();
    assert_eq!(loaded.len(), 365);
    let seen: HashSet<_> = loaded
        .into_iter()
        .filter_map(|score| score.tension)
        .collect();
    for idx in 0..writer_count {
        let key = format!("writer-{}", idx);
        assert!(seen.contains(&key), "missing {}", key);
    }
}

#[test]
fn test_load_recent_scores_reports_invalid_jsonl_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let valid = ScoreResult {
        layers: [
            LayerScore {
                name: "L1".into(),
                signal: Signal::Green,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L2".into(),
                signal: Signal::Yellow,
                indicators: Vec::new(),
            },
            LayerScore {
                name: "L3".into(),
                signal: Signal::Red,
                indicators: Vec::new(),
            },
        ],
        tension: None,
        timestamp: Utc::now(),
    };
    let valid_line = serde_json::to_string(&valid).unwrap();
    std::fs::write(&path, format!("{}\n{{\"bad\":\n", valid_line)).unwrap();

    let err = load_recent_scores_from_path(&path, 10).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("line 2"));
    assert!(msg.contains("score history"));
}

#[test]
fn test_legacy_unversioned_scores_are_activity_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let legacy_line = r#"{"layers":[{"name":"L1","signal":"Green","indicators":[]},{"name":"L2","signal":"Yellow","indicators":[]},{"name":"L3","signal":"Red","indicators":[]}],"tension":"legacy tension"}"#;
    std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

    let compatible = load_recent_scores_from_path(&path, 10).unwrap();
    assert!(compatible.is_empty());

    let activity = load_score_activity_from_path(&path, 10).unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].tension.as_deref(), Some("legacy tension"));
    assert_eq!(activity[0].timestamp, chrono::DateTime::<Utc>::UNIX_EPOCH);
}

#[test]
fn test_unversioned_current_indicator_contract_is_compatible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let mut score = make_score_result(
        4.0,
        80.0,
        20.0,
        0.5,
        12.0,
        25.0,
        1.0,
        30.0,
        6.0,
        0.2,
        0.8,
        Utc::now(),
    );
    remove_indicator(&mut score, "depth_output");
    remove_indicator(&mut score, "knowledge_rate");
    remove_indicator(&mut score, "friction_density");
    std::fs::write(
        &path,
        format!("{}\n", serde_json::to_string(&score).unwrap()),
    )
    .unwrap();

    let compatible = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(compatible.len(), 1);
}

#[test]
fn test_old_scoring_semantics_do_not_pollute_current_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let old = make_score_result(
        4.0,
        80.0,
        20.0,
        0.5,
        5.3,
        11.8,
        46.0,
        42.2,
        6.0,
        0.28,
        2.5,
        Utc::now() - Duration::days(1),
    );
    std::fs::write(&path, format!("{}\n", serde_json::to_string(&old).unwrap())).unwrap();

    let mut current = old.clone();
    remove_indicator(&mut current, "depth_output");
    remove_indicator(&mut current, "knowledge_rate");
    remove_indicator(&mut current, "friction_density");
    current.layers[1]
        .indicators
        .iter_mut()
        .find(|indicator| indicator.name == "fragmentation")
        .unwrap()
        .actual = 0.6;
    current.timestamp = Utc::now();
    persist_score_to_path(&path, &current).unwrap();

    let compatible = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(compatible.len(), 1);
    let fragmentation = compatible[0].layers[1]
        .indicators
        .iter()
        .find(|indicator| indicator.name == "fragmentation")
        .unwrap();
    assert_eq!(fragmentation.actual, 0.6);

    let activity = load_score_activity_from_path(&path, 10).unwrap();
    assert_eq!(activity.len(), 2, "legacy rows must remain auditable");
}
