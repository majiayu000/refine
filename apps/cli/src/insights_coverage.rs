//! Deterministic coverage disclosure for Session Insights.
//!
//! A report that was generated is not a report over complete data. This module
//! states when the report was made, which event time and sources it covers,
//! whether this run's ingest was complete, and whether the two windows are
//! comparable enough for any rise/fall conclusion.

use crate::insights_manifest::{InsightsManifest, WindowManifest};
use chrono::{DateTime, Local, Utc};
use std::collections::BTreeSet;

/// Two windows whose session counts differ by more than this factor are not
/// compared. One fixed ratio, not a scoring system.
const MAX_COMPARABLE_SESSION_RATIO: usize = 2;

/// Outcome of the ingest run that preceded this report, when the caller knows
/// it. A caller that knows nothing passes `None`: the report then cannot claim
/// complete collection and draws no rise/fall conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IngestRunStatus {
    /// The run finished with these failure and quarantine counts.
    Counted { failed: usize, quarantined: usize },
    /// The run reported incomplete collection but its counts could not be read.
    IncompleteUncounted,
}

impl IngestRunStatus {
    /// Map the `refine insights` ingest flags; no flags means unknown.
    pub(crate) fn from_flags(
        failed: Option<usize>,
        quarantined: Option<usize>,
        incomplete: bool,
    ) -> Option<Self> {
        if incomplete {
            return Some(Self::IncompleteUncounted);
        }
        failed
            .zip(quarantined)
            .map(|(failed, quarantined)| Self::Counted {
                failed,
                quarantined,
            })
    }

    fn is_incomplete(self) -> bool {
        match self {
            Self::Counted {
                failed,
                quarantined,
            } => failed > 0 || quarantined > 0,
            Self::IncompleteUncounted => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageAssessment {
    pub summary: String,
    pub trend_comparable: bool,
}

fn local(time: DateTime<Utc>) -> String {
    time.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn window_range(window: &WindowManifest) -> String {
    let start = window
        .event_time
        .start
        .map(local)
        .unwrap_or_else(|| "最早".to_string());
    let end = window
        .event_time
        .end
        .map(local)
        .unwrap_or_else(|| "现在".to_string());
    format!("{start} ～ {end}")
}

fn sessions(window: &WindowManifest) -> usize {
    window
        .source_counts
        .iter()
        .map(|source| source.session_count)
        .sum()
}

fn source_set(window: &WindowManifest) -> BTreeSet<&str> {
    window
        .source_counts
        .iter()
        .filter(|source| source.session_count > 0)
        .map(|source| source.source.as_str())
        .collect()
}

fn source_breakdown(window: &WindowManifest) -> String {
    if window.source_counts.is_empty() {
        return "无会话".to_string();
    }
    window
        .source_counts
        .iter()
        .map(|source| format!("{} {} 会话", source.source, source.session_count))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn freshest(window: &WindowManifest) -> String {
    let times: Vec<String> = window
        .source_counts
        .iter()
        .filter_map(|source| {
            source
                .freshest_event_time
                .map(|time| format!("{} {}", source.source, local(time)))
        })
        .collect();
    if times.is_empty() {
        "无".to_string()
    } else {
        times.join("；")
    }
}

fn ingest_line(ingest: Option<IngestRunStatus>) -> String {
    match ingest {
        None => "导入: 本报告未附带导入运行结果，不能确认本期采集完整".to_string(),
        Some(IngestRunStatus::IncompleteUncounted) => {
            "导入: 采集不完整——本次导入未完成，失败/隔离数未能读取；报告只基于已入库数据"
                .to_string()
        }
        Some(IngestRunStatus::Counted {
            failed: 0,
            quarantined: 0,
        }) => "导入: 本次导入无失败、无隔离（只说明本次运行，不代表历史全部覆盖）".to_string(),
        Some(IngestRunStatus::Counted {
            failed,
            quarantined,
        }) => format!(
            "导入: 采集不完整——本次导入失败 {failed}、隔离 {quarantined}；报告只基于已入库数据"
        ),
    }
}

/// Reasons the windows cannot support a rise/fall conclusion; empty when they can.
fn incomparable_reasons(
    current: &WindowManifest,
    previous: Option<&WindowManifest>,
    ingest: Option<IngestRunStatus>,
) -> Vec<String> {
    let Some(previous) = previous else {
        return vec!["全历史 snapshot，没有前一等长窗口".to_string()];
    };
    let mut reasons = Vec::new();
    if current.status != "OK" || previous.status != "OK" {
        reasons.push(format!(
            "数据质量 本期 {} / 上期 {}",
            current.status, previous.status
        ));
    }
    let (current_sources, previous_sources) = (source_set(current), source_set(previous));
    if current_sources != previous_sources {
        let join = |set: &BTreeSet<&str>| {
            if set.is_empty() {
                "无".to_string()
            } else {
                set.iter().copied().collect::<Vec<_>>().join("+")
            }
        };
        reasons.push(format!(
            "来源不同（本期 {} / 上期 {}）",
            join(&current_sources),
            join(&previous_sources)
        ));
    }
    let (now, before) = (sessions(current), sessions(previous));
    let (low, high) = (now.min(before), now.max(before));
    if low == 0 || high > low * MAX_COMPARABLE_SESSION_RATIO {
        reasons.push(format!(
            "会话数 {before}→{now} 相差超过 {MAX_COMPARABLE_SESSION_RATIO} 倍"
        ));
    }
    match ingest {
        None => reasons.push("本期导入状态未知，不能确认采集完整".to_string()),
        Some(status) if status.is_incomplete() => reasons.push("本期采集不完整".to_string()),
        Some(_) => {}
    }
    reasons
}

pub(crate) fn assess_coverage(
    manifest: &InsightsManifest,
    generated_at: DateTime<Utc>,
    ingest: Option<IngestRunStatus>,
) -> CoverageAssessment {
    let current = &manifest.current_window;
    let previous = manifest.previous_window.as_ref();
    let mut lines = vec![
        format!("- 报告生成时间: {}", local(generated_at)),
        format!(
            "- 事件覆盖: 本期 {}（按会话事件时间）；各来源最新事件 {}",
            window_range(current),
            freshest(current)
        ),
    ];
    match previous {
        Some(previous) => {
            lines.push(format!("- 上期事件覆盖: {}", window_range(previous)));
            lines.push(format!(
                "- 来源: 本期 {}；上期 {}",
                source_breakdown(current),
                source_breakdown(previous)
            ));
            let (now, before) = (source_set(current), source_set(previous));
            let missing_now: Vec<&str> = before.difference(&now).copied().collect();
            let missing_before: Vec<&str> = now.difference(&before).copied().collect();
            let mut gaps = Vec::new();
            if !missing_now.is_empty() {
                gaps.push(format!("本期缺 {}", missing_now.join("、")));
            }
            if !missing_before.is_empty() {
                gaps.push(format!("上期缺 {}", missing_before.join("、")));
            }
            lines.push(format!(
                "- 来源缺口: {}；来源不明会话 本期 {} / 上期 {}",
                if gaps.is_empty() {
                    "无".to_string()
                } else {
                    gaps.join("；")
                },
                current.platform_unknown_sessions,
                previous.platform_unknown_sessions
            ));
        }
        None => {
            lines.push(format!("- 来源: {}", source_breakdown(current)));
            lines.push(format!(
                "- 来源缺口: 来源不明会话 {}",
                current.platform_unknown_sessions
            ));
        }
    }
    lines.push(format!(
        "- 未纳入: 脱链 {}、无人值守/子代理 {}、非会话来源 {}、待核对修订 {}",
        current.detached_observations,
        current.mode_excluded_observations,
        current.source_excluded_observations,
        current.curation_excluded_observations
    ));
    lines.push(format!("- {}", ingest_line(ingest)));
    let reasons = incomparable_reasons(current, previous, ingest);
    let trend_comparable = reasons.is_empty();
    lines.push(if trend_comparable {
        "- 纵向可比: 可比（仍需同类任务前后证据才可写变化）".to_string()
    } else {
        format!(
            "- 纵向可比: 不可比（{}）；不输出认知升降结论，只描述本周表现",
            reasons.join("；")
        )
    });
    CoverageAssessment {
        summary: lines.join("\n"),
        trend_comparable,
    }
}

#[cfg(test)]
#[path = "insights_coverage_tests.rs"]
mod tests;
