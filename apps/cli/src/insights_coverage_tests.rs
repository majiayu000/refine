use super::*;
use crate::insights_manifest::{
    EventTimeWindow, SourceStats, UnsupportedSourceBreakdown, COHORT_CONTRACT_IDENTITY,
    MANIFEST_VERSION,
};
use chrono::TimeZone;

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2026, 10, day, hour, 0, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

fn window(start: u32, end: u32, sources: &[(&str, usize)]) -> WindowManifest {
    WindowManifest {
        event_time: EventTimeWindow {
            start: Some(at(start, 14)),
            end: Some(at(end, 14)),
        },
        input_observations: 10,
        linked_observations: 10,
        detached_observations: 0,
        mode_excluded_observations: 0,
        source_excluded_observations: 0,
        curation_excluded_observations: 0,
        eligible_observations: 10,
        ambiguous_project_alias_observations: 0,
        ambiguous_project_aliases: 0,
        linked_ratio: "1.000000".into(),
        status: "OK".into(),
        cohort_contract_identity: COHORT_CONTRACT_IDENTITY.into(),
        cohort_identity: "sha256:test".into(),
        source_counts: sources
            .iter()
            .map(|(source, sessions)| SourceStats {
                source: (*source).into(),
                observation_count: sessions * 8,
                session_count: *sessions,
                freshest_event_time: Some(at(end, 12)),
            })
            .collect(),
        unsupported_sources: UnsupportedSourceBreakdown::default(),
        platform_unknown_observations: 0,
        platform_unknown_sessions: 0,
    }
}

fn manifest(current: WindowManifest, previous: Option<WindowManifest>) -> InsightsManifest {
    InsightsManifest {
        manifest_version: MANIFEST_VERSION,
        mode: "rolling-7d-delta".into(),
        event_time_cutoff: at(23, 14),
        current_window: current,
        previous_window: previous,
        model_identity: "test".into(),
        prompt_identity: "test".into(),
        route_identity: "test".into(),
        binary_identity: "test".into(),
        source_revision: "test".into(),
    }
}

/// Issue #264 snapshot of 2026-10-23: 47 sessions from Claude and Codex
/// against 2 Codex sessions, after an ingest with 5 failures and 3 quarantined.
#[test]
fn issue_264_coverage_gap_is_incomparable_and_incomplete() {
    let report = manifest(
        window(16, 23, &[("claude", 30), ("codex", 17)]),
        Some(window(9, 16, &[("codex", 2)])),
    );
    let ingest = IngestRunStatus::Counted {
        failed: 5,
        quarantined: 3,
    };
    let coverage = assess_coverage(&report, at(23, 14), Some(ingest));
    assert!(!coverage.trend_comparable);
    let text = &coverage.summary;
    assert!(text.contains("- 报告生成时间: 2026-10-23 14:00"), "{text}");
    assert!(
        text.contains("本期 2026-10-16 14:00 ～ 2026-10-23 14:00（按会话事件时间）"),
        "{text}"
    );
    assert!(text.contains("- 上期事件覆盖: 2026-10-09 14:00 ～ 2026-10-16 14:00"));
    assert!(text.contains("本期 claude 30 会话 / codex 17 会话；上期 codex 2 会话"));
    assert!(text.contains("- 来源缺口: 上期缺 claude"));
    assert!(text.contains("采集不完整——本次导入失败 5、隔离 3"));
    assert!(text.contains("来源不同（本期 claude+codex / 上期 codex）"));
    assert!(text.contains("会话数 2→47 相差超过 2 倍"));
    assert!(text.contains("不输出认知升降结论"));
}

#[test]
fn similar_windows_with_complete_ingest_are_comparable() {
    let report = manifest(
        window(16, 23, &[("claude", 12), ("codex", 8)]),
        Some(window(9, 16, &[("claude", 9), ("codex", 6)])),
    );
    let clean = IngestRunStatus::Counted {
        failed: 0,
        quarantined: 0,
    };
    let coverage = assess_coverage(&report, at(23, 14), Some(clean));
    assert!(coverage.trend_comparable, "{}", coverage.summary);
    assert!(coverage.summary.contains("- 来源缺口: 无"));
    assert!(coverage.summary.contains("本次导入无失败、无隔离"));
    assert!(coverage.summary.contains("纵向可比: 可比"));
    assert!(!coverage.summary.contains("采集不完整"));
}

#[test]
fn partial_ingest_alone_blocks_comparison() {
    let report = manifest(
        window(16, 23, &[("codex", 10)]),
        Some(window(9, 16, &[("codex", 10)])),
    );
    let partial = IngestRunStatus::Counted {
        failed: 1,
        quarantined: 0,
    };
    let coverage = assess_coverage(&report, at(23, 14), Some(partial));
    assert!(!coverage.trend_comparable);
    assert!(coverage
        .summary
        .contains("采集不完整——本次导入失败 1、隔离 0"));
    assert!(coverage
        .summary
        .contains("纵向可比: 不可比（本期采集不完整）"));
}

/// Ingest reported incomplete collection but its counts could not be parsed:
/// the report still says collection is incomplete.
#[test]
fn incomplete_ingest_without_counts_is_still_incomplete() {
    let report = manifest(
        window(16, 23, &[("codex", 10)]),
        Some(window(9, 16, &[("codex", 10)])),
    );
    let coverage = assess_coverage(
        &report,
        at(23, 14),
        Some(IngestRunStatus::IncompleteUncounted),
    );
    assert!(!coverage.trend_comparable);
    assert!(coverage
        .summary
        .contains("采集不完整——本次导入未完成，失败/隔离数未能读取"));
    assert!(coverage
        .summary
        .contains("纵向可比: 不可比（本期采集不完整）"));
}

/// A manual `refine insights` run carries no ingest outcome: completeness is
/// unknown, so the windows are not compared.
#[test]
fn unknown_ingest_is_disclosed_and_not_comparable() {
    let report = manifest(
        window(16, 23, &[("codex", 10)]),
        Some(window(9, 16, &[("codex", 10)])),
    );
    let unknown = assess_coverage(&report, at(23, 14), None);
    assert!(!unknown.trend_comparable);
    assert!(unknown
        .summary
        .contains("未附带导入运行结果，不能确认本期采集完整"));
    assert!(unknown
        .summary
        .contains("纵向可比: 不可比（本期导入状态未知，不能确认采集完整）"));
    assert!(!unknown.summary.contains("本次导入无失败"));
}

#[test]
fn full_history_snapshot_is_never_compared() {
    let mut current = window(1, 23, &[("codex", 10)]);
    current.event_time.start = None;
    let coverage = assess_coverage(&manifest(current, None), at(23, 14), None);
    assert!(!coverage.trend_comparable);
    assert!(coverage.summary.contains("本期 最早 ～ 2026-10-23 14:00"));
    assert!(coverage.summary.contains("全历史 snapshot"));
}
