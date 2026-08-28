mod baseline;
mod compute;
mod display;
mod indicators;
mod persistence;
mod statusline;
pub(crate) mod streak;
mod types;

#[cfg(test)]
mod tests;

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use refine_core::knowledge::{Item, ItemRepository, ItemType};
use refine_core::session::{
    cluster_observations, cluster_observations_with_resolver, ProjectIdentityResolver,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use baseline::compute_personal_baseline;
pub use compute::compute;
pub use display::{indicator_display, layer_display};
pub use persistence::{load_recent_scores, persist_score};
pub use statusline::write_statusline;
pub use types::{Indicator, LayerScore, ScoreResult, Signal};

use baseline::compute_personal_trends;
use display::print_score;

#[cfg(test)]
use baseline::{trend_from_personal, PersonalBaseline};

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
    llm: Option<Arc<dyn refine_core::infra::LlmClient>>,
    since: Option<String>,
    all: bool,
    require_advice: bool,
    db_path: &Path,
) -> Result<()> {
    if all && since.is_some() {
        anyhow::bail!("--all and --since are mutually exclusive");
    }
    let now = Utc::now();
    let items = if all {
        repo.find_all()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    } else {
        let cutoff = if let Some(ref since_str) = since {
            chrono::NaiveDate::parse_from_str(since_str, "%Y-%m-%d")
                .map_err(|e| anyhow::anyhow!("invalid --since date '{}': {}", since_str, e))?
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| anyhow::anyhow!("invalid date"))?
                .and_utc()
        } else {
            now - chrono::Duration::days(90)
        };
        repo.find_observations_by_event_range(cutoff, now)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    };
    if items.is_empty() {
        return finish_without_observations(
            require_advice,
            crate::lang::t!(
                "No observation data. Run `refine ingest-sessions` first.",
                "暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            ),
        );
    }
    let obs_count = items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
        .count();
    if obs_count == 0 {
        return finish_without_observations(
            require_advice,
            crate::lang::t!(
                "No observation data in the time window. Run `refine ingest-sessions` first.",
                "当前时间窗口内无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            ),
        );
    }
    let cluster = cluster_observations(&items);
    if cluster.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "No eligible linked interactive observations in the score window (input {}, detached {}, mode-excluded {}); refusing to persist an empty score or generate advice",
            cluster.data_quality.input_observations,
            cluster.data_quality.detached_observations,
            cluster.data_quality.mode_excluded_observations,
        );
    }
    let config = crate::config::load();
    let result = compute(&cluster, &config.targets);

    // Try personal baseline: load history BEFORE persisting current score
    let history = load_recent_scores(365)?;
    let baseline = compute_personal_baseline(&history);
    persist_score(&result)?;
    let trends = baseline
        .as_ref()
        .map(|baseline| compute_personal_trends(&result, baseline));
    print_score(&result, trends.as_ref());

    let window = if all {
        crate::lang::t!("all observations", "全部观测").to_string()
    } else if let Some(since_date) = since.as_deref() {
        crate::lang::t!(
            format!("since {} (event time)", since_date),
            format!("自 {} 起(事件时间)", since_date)
        )
    } else {
        crate::lang::t!(
            "rolling 90 days (event time)".to_string(),
            "滚动 90 天(事件时间)".to_string()
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

    let mut advice_error = None;
    match compute_portfolio_advice_scores(&repo, now).await {
        Ok(portfolio) => {
            match crate::advice::cache_current_deterministic(
                &portfolio.long_term,
                &portfolio.recent,
                result.timestamp,
                &portfolio.long_cohort_identity,
                &portfolio.recent_cohort_identity,
            ) {
                Ok(_) => {}
                Err(error) => advice_error = Some(error),
            }
            if let Some(llm) = llm {
                match crate::advice::generate_and_cache(
                    &portfolio.long_term,
                    &portfolio.recent,
                    &llm,
                    result.timestamp,
                    &portfolio.long_cohort_identity,
                    &portfolio.recent_cohort_identity,
                )
                .await
                {
                    Ok(advice) => {
                        println!("\n  {} {}", crate::lang::t!("Advice:", "建议:"), advice)
                    }
                    Err(error) => {
                        tracing::error!("advice generation failed: {}", error);
                        advice_error = Some(error);
                    }
                }
            } else if require_advice {
                advice_error = Some(anyhow::anyhow!(
                    "LLM advice is required but no supported API key is configured"
                ));
            }
        }
        Err(error) => {
            tracing::error!("portfolio advice metrics failed: {}", error);
            if let Err(invalidation_error) = crate::advice::invalidate_cached() {
                advice_error = Some(invalidation_error.context(format!(
                    "portfolio metrics failed ({error}); stale advice cache also could not be invalidated"
                )));
            } else {
                advice_error = Some(error);
            }
        }
    }

    if let Err(e) = write_statusline(&result, db_path, trends.as_ref()) {
        tracing::warn!("failed to write statusline.txt: {}", e);
    }
    if require_advice {
        if let Some(error) = advice_error {
            return Err(error.context("required mirror advice generation failed"));
        }
    }
    Ok(())
}

struct PortfolioAdviceScores {
    long_term: ScoreResult,
    recent: ScoreResult,
    long_cohort_identity: String,
    recent_cohort_identity: String,
}

async fn compute_portfolio_advice_scores(
    repo: &Arc<dyn ItemRepository>,
    now: DateTime<Utc>,
) -> Result<PortfolioAdviceScores> {
    let long_term_items = repo
        .find_observations_by_event_range(
            now - chrono::Duration::days(crate::advice::LONG_TERM_WINDOW_DAYS),
            now,
        )
        .await
        .map_err(|error| anyhow::anyhow!("failed to load rolling-90-day advice cohort: {error}"))?;
    let recent_items = repo
        .find_observations_by_event_range(now - chrono::Duration::days(7), now)
        .await
        .map_err(|error| anyhow::anyhow!("failed to load rolling-7-day advice cohort: {error}"))?;

    let resolver = ProjectIdentityResolver::from_observation_windows(&[
        long_term_items.as_slice(),
        recent_items.as_slice(),
    ]);
    let long_term = cluster_observations_with_resolver(&long_term_items, &resolver);
    let recent = cluster_observations_with_resolver(&recent_items, &resolver);
    if long_term.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "portfolio advice requires eligible linked observations in the rolling-90-day window"
        );
    }
    if recent.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "portfolio advice requires eligible linked observations in the rolling-7-day window"
        );
    }

    let config = crate::config::load();
    Ok(PortfolioAdviceScores {
        long_term: compute(&long_term, &config.targets),
        recent: compute(&recent, &config.targets),
        long_cohort_identity: long_term.data_quality.cohort_identity,
        recent_cohort_identity: recent.data_quality.cohort_identity,
    })
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
