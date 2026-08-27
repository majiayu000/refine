use super::*;
use chrono::{Duration, Utc};
use std::collections::HashSet;
use std::sync::{Arc, Barrier};

use super::super::persistence::{
    load_score_activity_from_path, persist_score_to_path, SCORE_SCHEMA_VERSION,
};

fn score_line(score: &ScoreResult, schema_version: Option<u32>) -> String {
    let mut value = serde_json::to_value(score).unwrap();
    if let Some(version) = schema_version {
        value.as_object_mut().unwrap().insert(
            "score_schema_version".into(),
            serde_json::Value::from(version),
        );
    }
    serde_json::to_string(&value).unwrap()
}

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
    assert_eq!(SCORE_SCHEMA_VERSION, 4);

    let persisted: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(&path).unwrap().trim()).unwrap();
    assert_eq!(
        persisted["score_schema_version"],
        serde_json::Value::from(SCORE_SCHEMA_VERSION)
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
    std::fs::write(&path, format!("{}\n\n{{\"bad\":\n", valid_line)).unwrap();

    let err = load_recent_scores_from_path(&path, 10).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("line 3"));
    assert!(msg.contains("score history"));
}

#[test]
fn test_legacy_score_is_activity_only_and_accepts_missing_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");

    let legacy_line = r#"{"layers":[{"name":"L1","signal":"Green","indicators":[]},{"name":"L2","signal":"Yellow","indicators":[]},{"name":"L3","signal":"Red","indicators":[]}],"tension":"legacy tension"}"#;
    std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

    assert!(load_recent_scores_from_path(&path, 10).unwrap().is_empty());

    let activity = load_score_activity_from_path(&path, 10).unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].timestamp, chrono::DateTime::<Utc>::UNIX_EPOCH);
}

#[test]
fn test_known_old_schema_is_excluded_from_metrics_but_kept_as_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let old = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap());
    std::fs::write(&path, format!("{}\n", score_line(&old, Some(2)))).unwrap();

    assert!(load_recent_scores_from_path(&path, 10).unwrap().is_empty());
    assert_eq!(load_score_activity_from_path(&path, 10).unwrap().len(), 1);
}

#[test]
fn linked_only_v4_excludes_mixed_cohort_v3_from_metrics_but_keeps_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let mixed_cohort_v3 = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 8, 20).unwrap());
    let linked_only_v4 = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 8, 27).unwrap());
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n",
            score_line(&mixed_cohort_v3, Some(3)),
            score_line(&linked_only_v4, Some(4)),
        ),
    )
    .unwrap();

    let metrics = load_recent_scores_from_path(&path, 10).unwrap();
    assert_eq!(metrics.len(), 1);
    assert_eq!(metrics[0].timestamp, linked_only_v4.timestamp);

    let activity = load_score_activity_from_path(&path, 10).unwrap();
    assert_eq!(activity.len(), 2);
    assert_eq!(activity[0].timestamp, mixed_cohort_v3.timestamp);
    assert_eq!(activity[1].timestamp, linked_only_v4.timestamp);
}

#[test]
fn test_recent_limit_is_applied_after_schema_filtering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let old_unversioned = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 20).unwrap());
    let old_v2 = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n",
            score_line(&old_unversioned, None),
            score_line(&old_v2, Some(2))
        ),
    )
    .unwrap();

    for (date, tension) in [(22, "current-1"), (23, "current-2"), (24, "current-3")] {
        let mut score = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, date).unwrap());
        score.tension = Some(tension.into());
        persist_score_to_path(&path, &score).unwrap();
    }

    let loaded = load_recent_scores_from_path(&path, 2).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].tension.as_deref(), Some("current-2"));
    assert_eq!(loaded[1].tension.as_deref(), Some("current-3"));
}

#[test]
fn test_future_schema_errors_for_metrics_but_remains_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let timestamp = chrono::Utc.with_ymd_and_hms(2026, 3, 20, 12, 0, 0).unwrap();
    let future_line = serde_json::json!({
        "score_schema_version": 99,
        "timestamp": timestamp,
        "layers": {"future_shape": true},
        "metric_payload": ["not", "compatible"]
    })
    .to_string();
    std::fs::write(&path, format!("{}\n", future_line)).unwrap();

    let error = load_recent_scores_from_path(&path, 10).unwrap_err();
    assert!(error
        .to_string()
        .contains("unsupported score schema version 99"));

    let activity = load_score_activity_from_path(&path, 10).unwrap();
    assert_eq!(activity.len(), 1);
    assert_eq!(activity[0].timestamp, timestamp);

    let before = std::fs::read_to_string(&path).unwrap();
    let current = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 21).unwrap());
    let append_error = persist_score_to_path(&path, &current).unwrap_err();
    assert!(append_error
        .to_string()
        .contains("unsupported score schema version 99"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}
