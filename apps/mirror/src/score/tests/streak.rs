use super::*;

#[test]
fn test_streak_zero_records() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap();
    assert_eq!(calculate_streak(&[], today), 0);
}

#[test]
fn test_streak_consecutive_3_days() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap();
    let scores = vec![
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 24).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap()),
    ];
    assert_eq!(calculate_streak(&scores, today), 3);
}

#[test]
fn test_streak_gap_resets() {
    // 3/24, 3/25, (gap 3/26), 3/27 = today -> streak = 1
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 27).unwrap();
    let scores = vec![
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 24).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 27).unwrap()),
    ];
    assert_eq!(calculate_streak(&scores, today), 1);
}

#[test]
fn test_streak_duplicate_dates_counted_once() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap();
    let scores = vec![
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap()),
    ];
    assert_eq!(calculate_streak(&scores, today), 1);
}

#[test]
fn test_streak_spans_old_and_current_score_schemas() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scores.jsonl");
    let old = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap());
    std::fs::write(&path, format!("{}\n", serde_json::to_string(&old).unwrap())).unwrap();
    let current = make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap());
    super::super::persistence::persist_score_to_path(&path, &current).unwrap();

    let activity = super::super::persistence::load_score_activity_from_path(&path, 365).unwrap();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap();
    assert_eq!(calculate_streak(&activity, today), 2);
}

#[test]
fn test_streak_no_record_today() {
    // Records on 3/24, 3/25 but today is 3/26 -> streak = 0
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 26).unwrap();
    let scores = vec![
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 24).unwrap()),
        make_score_at_date(chrono::NaiveDate::from_ymd_opt(2026, 3, 25).unwrap()),
    ];
    assert_eq!(calculate_streak(&scores, today), 0);
}

#[test]
fn test_milestone_messages() {
    assert_eq!(
        milestone_message(7),
        Some("\u{1f3af} 一周连续！习惯正在形成")
    );
    assert_eq!(
        milestone_message(30),
        Some("\u{1f3c6} 连续一个月！认知追踪已成习惯")
    );
    assert_eq!(milestone_message(100), Some("\u{1f4af} 百日里程碑！"));
    assert_eq!(milestone_message(365), Some("\u{1f38a} 整整一年！"));
    assert_eq!(milestone_message(5), None);
    assert_eq!(milestone_message(0), None);
}

#[test]
fn test_format_streak_display() {
    assert_eq!(format_streak(0), None);
    assert_eq!(format_streak(1), None);
    assert_eq!(format_streak(2), Some("\u{1f525} 连续 2 天".to_string()));
    assert_eq!(format_streak(30), Some("\u{1f525} 连续 30 天".to_string()));
}
