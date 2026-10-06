mod baseline;
mod compute;
mod display;
mod indicators;
mod persistence;
mod publication;
mod scope;
mod statusline;
pub(crate) mod streak;
mod types;

#[cfg(test)]
mod tests;

use crate::cohort::{load_cohorts, EventWindow};
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use refine_core::knowledge::{Item, ItemRepository};
use refine_core::session::format_data_quality_stats;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use baseline::compute_personal_baseline;
pub use compute::compute;
pub use display::{indicator_display, layer_display};
pub use persistence::load_recent_scores;
pub(crate) use publication::{
    cache_belongs_to_score, invalidate_empty_score_cache, publish_canonical_score,
};
pub use scope::ScoreScope;
pub use types::{Indicator, LayerScore, ScoreResult, Signal};

use display::print_score;

#[cfg(test)]
use baseline::{compute_personal_trends, trend_from_personal, PersonalBaseline};

#[cfg(test)]
use types::Trend;

#[cfg(test)]
use compute::{analyze_tension, dreyfus_weighted, layer1, layer3};

#[cfg(test)]
use persistence::load_recent_scores_from_path;

#[cfg(test)]
use streak::{calculate_streak, format_streak, milestone_message};

/// Filter items to only those created since the given date string (YYYY-MM-DD).
/// If `since` is None, returns all items unchanged.
// Preserved for use in tests and potential future callers.
#[allow(dead_code)]
pub fn filter_since(items: Vec<Item>, since: &Option<String>) -> Result<Vec<Item>> {
    let Some(since_str) = since.as_deref() else {
        return Ok(items);
    };
    let date = NaiveDate::parse_from_str(since_str, "%Y-%m-%d")
        .map_err(|e| anyhow::anyhow!("invalid --since date '{}': {}", since_str, e))?;
    let cutoff = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid date"))?
        .and_utc();
    Ok(items
        .into_iter()
        .filter(|i| i.created_at() >= cutoff)
        .collect())
}

// ── CLI handler ──

pub async fn handle_score(
    repo: Arc<dyn ItemRepository>,
    since: Option<String>,
    all: bool,
    require_advice: bool,
    db_path: &Path,
    cache_dir: &Path,
) -> Result<()> {
    let canonical = !all && since.is_none();
    if require_advice && !canonical {
        anyhow::bail!("--require-advice requires the default rolling-90-day score");
    }
    let now = Utc::now();
    let selected = EventWindow::from_options(since.as_deref(), all, now)?;
    let windows = if canonical {
        vec![selected, EventWindow::rolling(7, now)]
    } else {
        vec![selected]
    };
    let mut cohorts = load_cohorts(repo.as_ref(), now, &windows)
        .await?
        .into_iter();
    let selected = cohorts.next().expect("selected score window");
    let cluster = &selected.cluster;
    let items = &selected.cohort_items;
    let config = crate::config::load();
    let scope = canonical
        .then(|| ScoreScope::canonical(db_path, &config.targets, now, &cluster.data_quality))
        .transpose()?;
    if cluster.data_quality.input_observations == 0 {
        invalidate_empty_score_cache(scope.as_ref(), cache_dir)?;
        return finish_without_observations(
            require_advice,
            crate::lang::t!(
                "No observation data in the time window. Run `refine ingest-sessions` first.",
                "当前时间窗口内无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            ),
        );
    }
    if cluster.data_quality.eligible_observations == 0 {
        invalidate_empty_score_cache(scope.as_ref(), cache_dir)?;
        anyhow::bail!(
            "No eligible linked interactive observations in the score window ({}); refusing to persist an empty score or generate advice",
            format_data_quality_stats(&cluster.data_quality),
        );
    }
    let mut result = compute(cluster, &config.targets);
    result.timestamp = now;
    result.scope = scope;
    let published = if canonical {
        let recent = cohorts.next().expect("canonical recent advice window");
        let recent_score = compute(&recent.cluster, &config.targets);
        Some(publish_canonical_score(
            &result,
            &recent_score,
            &recent.cluster.data_quality.cohort_identity,
            cache_dir,
        )?)
    } else {
        None
    };
    let trends = published
        .as_ref()
        .and_then(|published| published.trends.as_ref());
    print_score(&result, trends);

    println!(
        "  {} {}",
        crate::lang::t!("Data quality:", "数据质量:"),
        format_data_quality_stats(&cluster.data_quality)
    );

    let window = if all {
        crate::lang::t!(
            "all session observations (view only)",
            "全部会话观测(仅查看)"
        )
        .to_string()
    } else if let Some(since_date) = since.as_deref() {
        crate::lang::t!(
            format!("since {} (session start; view only)", since_date),
            format!("自 {} 起(会话开始时间；仅查看)", since_date)
        )
    } else {
        crate::lang::t!(
            "rolling 90 days (session start)".to_string(),
            "滚动 90 天(会话开始时间)".to_string()
        )
    };
    println!("  {} {}", crate::lang::t!("Window:", "窗口:"), window);

    // Items expose persistence timestamps here; the selection window above is
    // based on the source document's event timestamp.
    if !items.is_empty() {
        let (min_t, max_t) = items.iter().fold(
            (DateTime::<Utc>::MAX_UTC, DateTime::<Utc>::MIN_UTC),
            |(min, max), item| {
                let t = item.created_at();
                (if t < min { t } else { min }, if t > max { t } else { max })
            },
        );
        println!(
            "  {} {} ~ {}",
            crate::lang::t!("Stored item range:", "入库条目范围:"),
            min_t.format("%Y-%m-%d"),
            max_t.format("%Y-%m-%d"),
        );
    }

    // Check for pending ingest from growth-tracker
    let tracker_path = resolve_growth_tracker_path(db_path);
    if let Ok(content) = std::fs::read_to_string(&tracker_path) {
        if let Ok(tracker) = serde_json::from_str::<serde_json::Value>(&content) {
            let pending = tracker
                .get("pending_ingest")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if pending > 3 {
                println!(
                    "  ⚠️ {} {} {}",
                    crate::lang::t!("There are", "有"),
                    pending,
                    crate::lang::t!(
                        "sessions not yet analyzed. Run: refine ingest-sessions",
                        "个 session 未分析。运行: refine ingest-sessions"
                    )
                );
            }
        }
    }

    if !canonical {
        println!(
            "  {}",
            crate::lang::t!(
                "View only: canonical history and portfolio advice are unchanged.",
                "仅查看：不更新标准评分历史和项目建议。"
            )
        );
        return Ok(());
    }

    match published.expect("canonical score publication").advice {
        Ok(advice) => println!("\n  {} {}", crate::lang::t!("Advice:", "建议:"), advice),
        Err(error) if require_advice => {
            return Err(error.context("required mirror advice generation failed"))
        }
        Err(_) => {} // The shared publisher already reports the local cache error.
    }
    Ok(())
}

fn finish_without_observations(require_advice: bool, message: &str) -> Result<()> {
    println!("{message}");
    if require_advice {
        anyhow::bail!("required mirror advice cannot be generated without observation data");
    }
    Ok(())
}

fn resolve_growth_tracker_path(db_path: &Path) -> PathBuf {
    let primary = growth_tracker_path_from_db(db_path);
    let legacy = dirs::home_dir().map(|home| home.join(".refine").join("growth-tracker.json"));
    choose_growth_tracker_path(primary, legacy)
}

fn growth_tracker_path_from_db(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("growth-tracker.json")
}

fn choose_growth_tracker_path(primary: PathBuf, legacy: Option<PathBuf>) -> PathBuf {
    if primary.exists() {
        return primary;
    }
    if let Some(legacy_path) = legacy {
        if legacy_path.exists() {
            return legacy_path;
        }
    }
    primary
}
