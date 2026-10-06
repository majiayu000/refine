mod baseline;
mod compute;
mod display;
mod indicators;
mod persistence;
mod scope;
mod statusline;
pub(crate) mod streak;
mod types;

#[cfg(test)]
mod tests;

use crate::cohort::{load_cohorts, EventWindow};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use refine_core::knowledge::{Item, ItemRepository};
use refine_core::session::format_data_quality_stats;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use baseline::compute_personal_baseline;
pub use compute::compute;
pub use display::{indicator_display, layer_display};
pub use persistence::{load_recent_scores, persist_score};
pub use scope::ScoreScope;
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
    if cluster.data_quality.input_observations == 0 {
        invalidate_empty_score_cache(canonical, cache_dir)?;
        return finish_without_observations(
            require_advice,
            crate::lang::t!(
                "No observation data in the time window. Run `refine ingest-sessions` first.",
                "当前时间窗口内无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            ),
        );
    }
    if cluster.data_quality.eligible_observations == 0 {
        invalidate_empty_score_cache(canonical, cache_dir)?;
        anyhow::bail!(
            "No eligible linked interactive observations in the score window ({}); refusing to persist an empty score or generate advice",
            format_data_quality_stats(&cluster.data_quality),
        );
    }
    let config = crate::config::load();
    let mut result = compute(cluster, &config.targets);
    result.timestamp = now;
    let published = if canonical {
        result.scope = Some(ScoreScope::canonical(
            db_path,
            &config.targets,
            now,
            &cluster.data_quality,
        )?);
        let recent = cohorts.next().expect("canonical recent advice window");
        let recent_score = compute(&recent.cluster, &config.targets);
        Some(publish_canonical_score(
            &result,
            &recent_score,
            &recent.cluster.data_quality.cohort_identity,
            db_path,
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

pub(crate) struct PublishedScore {
    pub trends: Option<baseline::PersonalTrends>,
    pub advice: Result<String>,
}

/// Publish a canonical snapshot and its dependent output together for both
/// score and dashboard. Serialize writers so an older process cannot overwrite
/// cache/statusline output after a newer daily snapshot has been retained.
pub(crate) fn publish_canonical_score(
    result: &ScoreResult,
    recent: &ScoreResult,
    recent_cohort_identity: &str,
    db_path: &Path,
) -> Result<PublishedScore> {
    let scope = result
        .scope
        .as_ref()
        .context("canonical publication requires a score scope")?;
    let dir = crate::config::ensure_mirror_dir()?;
    with_publication_lock(&dir, || {
        let history = load_recent_scores(365, scope)?;
        let baseline = compute_personal_baseline(&history, scope);
        let trends = baseline
            .as_ref()
            .map(|baseline| compute_personal_trends(result, baseline));
        let retained = persist_score(result)?;
        let advice = if retained {
            match crate::advice::cache_current_deterministic(
                result,
                recent,
                result.timestamp,
                &scope.cohort_identity,
                recent_cohort_identity,
            ) {
                Ok(advice) => Ok(advice),
                Err(error) => {
                    tracing::error!("portfolio advice failed: {}", error);
                    Err(match crate::advice::invalidate_cached() {
                        Ok(()) => error,
                        Err(invalidation) => invalidation.context(format!(
                            "portfolio advice failed ({error}); stale advice cache also could not be invalidated"
                        )),
                    })
                }
            }
        } else {
            // A newer same-day snapshot is already retained. Render this view
            // locally while preserving the retained snapshot's derived files.
            crate::advice::portfolio_policy(result, recent)
                .map(|policy| crate::advice::deterministic_advice(&policy))
        };
        if retained {
            if let Err(error) = write_statusline(result, db_path, trends.as_ref()) {
                tracing::warn!("failed to write statusline.txt: {}", error);
            }
        }
        Ok(PublishedScore { trends, advice })
    })
}

fn with_publication_lock<T>(directory: &Path, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let path = directory.join("score-publication.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open canonical publication lock {}", path.display()))?;
    fs2::FileExt::lock_exclusive(&lock).context("acquire canonical publication lock")?;
    let result = work();
    let unlocked = fs2::FileExt::unlock(&lock).context("release canonical publication lock");
    let value = result?;
    unlocked?;
    Ok(value)
}

pub(crate) fn invalidate_empty_score_cache(canonical: bool, cache_dir: &Path) -> Result<()> {
    // A custom display window says nothing about the full portfolio cohort.
    if !canonical || !cache_dir.exists() {
        return Ok(());
    }
    with_publication_lock(cache_dir, || {
        for filename in ["advice.json", "statusline.txt"] {
            let path = cache_dir.join(filename);
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "failed to invalidate empty-score cache {}: {error}",
                        path.display()
                    ));
                }
            }
        }
        Ok(())
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
