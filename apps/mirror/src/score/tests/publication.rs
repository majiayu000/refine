use super::*;
use crate::advice::load_cached_for_score_in;
use std::path::Path;

fn timestamp(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, day, hour, 0, 0).unwrap()
}

fn score(database: &str, cutoff: DateTime<Utc>, degraded: bool) -> ScoreResult {
    let mut result = make_score_result(
        3.0, 30.0, 50.0, 15.0, 20.0, 60.0, 10.0, 30.0, 4.0, 0.2, 5.0, cutoff,
    );
    let quality = DataQualityStats {
        input_observations: if degraded { 2 } else { 1 },
        linked_observations: if degraded { 2 } else { 1 },
        eligible_observations: 1,
        curation_excluded_observations: usize::from(degraded),
        cohort_identity: format!("sha256:{}", if degraded { "b" } else { "a" }.repeat(64)),
        ..Default::default()
    };
    result.scope = Some(
        ScoreScope::canonical(
            Path::new(database),
            &crate::config::Targets::default(),
            cutoff,
            &quality,
        )
        .unwrap(),
    );
    result
}

fn empty_scope(database: &str, cutoff: DateTime<Utc>) -> ScoreScope {
    // Curation changes quality and cohort identity but not comparison scope.
    let quality = DataQualityStats {
        input_observations: 1,
        linked_observations: 1,
        curation_excluded_observations: 1,
        cohort_identity: format!("sha256:{}", "c".repeat(64)),
        ..Default::default()
    };
    ScoreScope::canonical(
        Path::new(database),
        &crate::config::Targets::default(),
        cutoff,
        &quality,
    )
    .unwrap()
}

fn publish(directory: &Path, result: &ScoreResult) {
    publish_canonical_score(
        result,
        result,
        &result.scope.as_ref().unwrap().cohort_identity,
        directory,
    )
    .unwrap()
    .advice
    .unwrap();
}

fn outputs(directory: &Path) -> (Vec<u8>, Vec<u8>) {
    (
        std::fs::read(directory.join("advice.json")).unwrap(),
        std::fs::read(directory.join("statusline.txt")).unwrap(),
    )
}

fn assert_no_outputs(directory: &Path) {
    assert!(!directory.join("advice.json").exists());
    assert!(!directory.join("statusline.txt").exists());
}

fn history(directory: &Path) -> Vec<ScoreResult> {
    load_recent_scores_from_path(&directory.join("scores.jsonl"), usize::MAX).unwrap()
}

#[test]
fn newer_empty_fences_late_valid_across_dates_and_new_valid_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let database = "/synthetic/a.db";
    let seed = score(database, timestamp(1, 10), false);
    let late = score(database, timestamp(2, 10), false);
    publish(dir.path(), &seed);
    let previous_history = std::fs::read(dir.path().join("scores.jsonl")).unwrap();

    // The late writer already holds its old nonempty snapshot. A newer empty
    // snapshot completes first, without adding a zero/unknown score to history.
    invalidate_empty_score_cache(Some(&empty_scope(database, timestamp(2, 11))), dir.path())
        .unwrap();
    let fence = std::fs::read(dir.path().join("score-publication.json")).unwrap();
    assert_no_outputs(dir.path());
    publish(dir.path(), &late);
    assert_no_outputs(dir.path());
    assert_eq!(
        std::fs::read(dir.path().join("scores.jsonl")).unwrap(),
        previous_history
    );
    assert_eq!(
        std::fs::read(dir.path().join("score-publication.json")).unwrap(),
        fence
    );

    let recovered = score(database, timestamp(2, 12), false);
    publish(dir.path(), &recovered);
    assert_eq!(history(dir.path()).len(), 2);
    assert_eq!(
        history(dir.path()).last().unwrap().timestamp,
        recovered.timestamp
    );
    let cached = load_cached_for_score_in(dir.path(), &recovered)
        .unwrap()
        .unwrap();
    assert!(String::from_utf8(outputs(dir.path()).1)
        .unwrap()
        .contains(&cached.short));
}

#[test]
fn first_empty_creates_fence_without_history_or_existing_directory() {
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("first-publication");
    let database = "/synthetic/a.db";
    assert!(!directory.exists());
    invalidate_empty_score_cache(Some(&empty_scope(database, timestamp(2, 11))), &directory)
        .unwrap();
    assert!(directory.join("score-publication.json").is_file());
    assert!(!directory.join("scores.jsonl").exists());
    publish(&directory, &score(database, timestamp(2, 10), false));
    assert_no_outputs(&directory);
    assert!(!directory.join("scores.jsonl").exists());
}

#[test]
fn newer_valid_fences_late_and_equal_cutoff_empty() {
    let dir = tempfile::tempdir().unwrap();
    let database = "/synthetic/a.db";
    let current = score(database, timestamp(2, 11), false);
    publish(dir.path(), &current);
    let expected = outputs(dir.path());
    let expected_history = std::fs::read(dir.path().join("scores.jsonl")).unwrap();
    let expected_fence = std::fs::read(dir.path().join("score-publication.json")).unwrap();
    for cutoff in [timestamp(1, 10), current.timestamp] {
        invalidate_empty_score_cache(Some(&empty_scope(database, cutoff)), dir.path()).unwrap();
        assert_eq!(outputs(dir.path()), expected);
        assert_eq!(
            std::fs::read(dir.path().join("scores.jsonl")).unwrap(),
            expected_history
        );
        assert_eq!(
            std::fs::read(dir.path().join("score-publication.json")).unwrap(),
            expected_fence
        );
    }
    assert!(load_cached_for_score_in(dir.path(), &current)
        .unwrap()
        .is_some());
}

#[test]
fn other_scope_can_record_history_but_late_valid_and_empty_preserve_cache_owner() {
    let dir = tempfile::tempdir().unwrap();
    let current = score("/synthetic/b.db", timestamp(2, 11), false);
    publish(dir.path(), &current);
    let expected = outputs(dir.path());
    let late = score("/synthetic/a.db", timestamp(2, 10), false);
    publish(dir.path(), &late);
    assert_eq!(history(dir.path()).len(), 2);
    assert_eq!(outputs(dir.path()), expected);
    assert!(load_cached_for_score_in(dir.path(), &current)
        .unwrap()
        .is_some());
    assert!(load_cached_for_score_in(dir.path(), &late)
        .unwrap()
        .is_none());

    for cutoff in [timestamp(2, 9), timestamp(2, 12)] {
        invalidate_empty_score_cache(Some(&empty_scope("/synthetic/a.db", cutoff)), dir.path())
            .unwrap();
        assert_eq!(outputs(dir.path()), expected);
        assert_eq!(history(dir.path()).len(), 2);
    }
    // A's newer empty result neither transfers cache ownership nor blocks B's
    // own next publication, even when B's cutoff is earlier than A's empty one.
    let refreshed = score(
        "/synthetic/b.db",
        timestamp(2, 11) + chrono::Duration::minutes(30),
        false,
    );
    publish(dir.path(), &refreshed);
    assert!(load_cached_for_score_in(dir.path(), &refreshed)
        .unwrap()
        .is_some());
    let expected = outputs(dir.path());
    publish(
        dir.path(),
        &score("/synthetic/a.db", timestamp(2, 11), false),
    );
    assert_eq!(outputs(dir.path()), expected);
    let recovered = score("/synthetic/a.db", timestamp(2, 13), false);
    publish(dir.path(), &recovered);
    assert!(load_cached_for_score_in(dir.path(), &recovered)
        .unwrap()
        .is_some());
    assert!(load_cached_for_score_in(dir.path(), &refreshed)
        .unwrap()
        .is_none());
}

#[test]
fn equal_cutoff_keeps_current_owner_across_scopes_and_within_scope() {
    let dir = tempfile::tempdir().unwrap();
    let first = score("/synthetic/a.db", timestamp(2, 11), false);
    let other = score("/synthetic/b.db", first.timestamp, false);
    publish(dir.path(), &first);
    let expected = outputs(dir.path());
    publish(dir.path(), &other);
    assert_eq!(history(dir.path()).len(), 2);
    assert_eq!(outputs(dir.path()), expected);
    assert!(load_cached_for_score_in(dir.path(), &first)
        .unwrap()
        .is_some());
    assert!(load_cached_for_score_in(dir.path(), &other)
        .unwrap()
        .is_none());
    let previous_history = std::fs::read(dir.path().join("scores.jsonl")).unwrap();
    publish(dir.path(), &score("/synthetic/a.db", first.timestamp, true));
    assert_eq!(outputs(dir.path()), expected);
    assert_eq!(
        std::fs::read(dir.path().join("scores.jsonl")).unwrap(),
        previous_history
    );
}

#[test]
fn newer_nonempty_quality_change_publishes_and_late_writer_cannot_undo_it() {
    for (old_degraded, new_degraded) in [(false, true), (true, false)] {
        let dir = tempfile::tempdir().unwrap();
        let older = score("/synthetic/a.db", timestamp(2, 10), old_degraded);
        let newer = score("/synthetic/a.db", timestamp(2, 11), new_degraded);
        publish(dir.path(), &older);
        publish(dir.path(), &newer);
        let expected = outputs(dir.path());
        publish(dir.path(), &older);
        assert_eq!(outputs(dir.path()), expected);
        let rows = history(dir.path());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp, newer.timestamp);
        assert_eq!(
            rows[0].scope.as_ref().unwrap().data_quality.is_degraded(),
            new_degraded
        );
        assert!(load_cached_for_score_in(dir.path(), &newer)
            .unwrap()
            .is_some());
    }
}

#[test]
fn existing_history_and_cache_bootstrap_ordering_and_unique_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let current = score("/synthetic/a.db", timestamp(2, 11), false);
    publish(dir.path(), &current);
    let expected = outputs(dir.path());
    // Simulate the preceding release: real score/advice files, no watermark.
    std::fs::remove_file(dir.path().join("score-publication.json")).unwrap();
    assert!(load_cached_for_score_in(dir.path(), &current)
        .unwrap()
        .is_some());
    invalidate_empty_score_cache(
        Some(&empty_scope("/synthetic/a.db", timestamp(2, 10))),
        dir.path(),
    )
    .unwrap();
    assert_eq!(outputs(dir.path()), expected);
    publish(
        dir.path(),
        &score("/synthetic/b.db", timestamp(2, 10), false),
    );
    assert_eq!(outputs(dir.path()), expected);
    assert!(dir.path().join("score-publication.json").exists());
    assert!(load_cached_for_score_in(dir.path(), &current)
        .unwrap()
        .is_some());
}

#[test]
fn ambiguous_legacy_cache_is_not_assigned_to_a_database_by_timestamp_alone() {
    let dir = tempfile::tempdir().unwrap();
    let first = score("/synthetic/a.db", timestamp(2, 11), false);
    let other = score("/synthetic/b.db", first.timestamp, false);
    publish(dir.path(), &first);
    super::super::persistence::persist_score_to_path(&dir.path().join("scores.jsonl"), &other)
        .unwrap();
    std::fs::remove_file(dir.path().join("score-publication.json")).unwrap();
    assert!(load_cached_for_score_in(dir.path(), &first)
        .unwrap()
        .is_none());
    assert!(load_cached_for_score_in(dir.path(), &other)
        .unwrap()
        .is_none());
    let previous_history = std::fs::read(dir.path().join("scores.jsonl")).unwrap();
    invalidate_empty_score_cache(
        Some(&empty_scope("/synthetic/a.db", timestamp(2, 12))),
        dir.path(),
    )
    .unwrap();
    assert_no_outputs(dir.path());
    assert_eq!(
        std::fs::read(dir.path().join("scores.jsonl")).unwrap(),
        previous_history
    );
}
