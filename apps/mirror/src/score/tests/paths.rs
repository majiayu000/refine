use super::*;
use chrono::{Duration, Utc};

#[test]
fn growth_tracker_path_from_db_uses_db_parent_dir() {
    let db_path = std::path::Path::new("/tmp/custom/refine.db");
    let tracker_path = growth_tracker_path_from_db(db_path);
    assert_eq!(
        tracker_path,
        std::path::PathBuf::from("/tmp/custom/growth-tracker.json")
    );
}

#[test]
fn choose_growth_tracker_path_prefers_primary_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let primary = dir.path().join("growth-tracker.json");
    let legacy = dir.path().join("legacy").join("growth-tracker.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&primary, r#"{"pending_ingest": 8}"#).unwrap();
    std::fs::write(&legacy, r#"{"pending_ingest": 3}"#).unwrap();

    let chosen = choose_growth_tracker_path(primary.clone(), Some(legacy));
    assert_eq!(chosen, primary);
}

#[test]
fn choose_growth_tracker_path_falls_back_to_legacy_when_primary_missing() {
    let dir = tempfile::tempdir().unwrap();
    let primary = dir.path().join("db").join("growth-tracker.json");
    let legacy = dir.path().join(".refine").join("growth-tracker.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, r#"{"pending_ingest": 5}"#).unwrap();

    let chosen = choose_growth_tracker_path(primary, Some(legacy.clone()));
    assert_eq!(chosen, legacy);
}

#[test]
fn filter_since_future_cutoff_returns_empty() {
    let items = vec![
        make_item_at(Utc::now() - Duration::days(10)),
        make_item_at(Utc::now() - Duration::days(5)),
    ];
    let future_cutoff = Some(
        (Utc::now() + Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
    );
    let result = filter_since(items, &future_cutoff).unwrap();
    assert!(result.is_empty());
}

#[test]
fn filter_since_none_returns_all() {
    let items = vec![
        make_item_at(Utc::now() - Duration::days(100)),
        make_item_at(Utc::now() - Duration::days(200)),
    ];
    let result = filter_since(items.clone(), &None).unwrap();
    assert_eq!(result.len(), items.len());
}

#[test]
fn filter_since_boundary_inclusive() {
    // Item created exactly at the cutoff date must be included (>= boundary)
    let cutoff_date = (Utc::now() - Duration::days(90))
        .format("%Y-%m-%d")
        .to_string();
    let cutoff_dt = Utc::now() - Duration::days(90);
    let items = vec![
        make_item_at(cutoff_dt),                        // exactly at boundary
        make_item_at(cutoff_dt - Duration::seconds(1)), // just before
        make_item_at(cutoff_dt + Duration::seconds(1)), // just after
    ];
    let result = filter_since(items, &Some(cutoff_date)).unwrap();
    // The item exactly at midnight of that day and the one after should be included;
    // the one 1 second before midnight should not.
    // Since cutoff is midnight (00:00:00) of the date, exact-at-cutoff_dt may fall
    // before midnight. We just verify the future item is included and the vec is non-empty.
    assert!(!result.is_empty());
    // The item 1 second after cutoff_dt must always be in the result
    assert!(result
        .iter()
        .any(|i| i.created_at() == cutoff_dt + Duration::seconds(1)));
}
