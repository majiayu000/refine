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
use refine_core::session::cluster_observations;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use baseline::compute_personal_baseline;
pub use compute::compute;
pub use display::{indicator_display, layer_display};
pub use persistence::{load_recent_scores, persist_score};
pub use statusline::write_statusline;
pub use types::{Indicator, LayerScore, ScoreResult, Signal};

use baseline::apply_personal_baseline;
use display::print_score;

#[cfg(test)]
use baseline::{signal_from_personal, PersonalBaseline};

#[cfg(test)]
use compute::{
    analyze_tension, dreyfus_weighted, friction_density, knowledge_rate, layer1, layer3,
};

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
    db_path: &Path,
) -> Result<()> {
    if all && since.is_some() {
        anyhow::bail!("--all and --since are mutually exclusive");
    }
    let items = if all {
        repo.find_all()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    } else if let Some(ref since_str) = since {
        let date = chrono::NaiveDate::parse_from_str(since_str, "%Y-%m-%d")
            .map_err(|e| anyhow::anyhow!("invalid --since date '{}': {}", since_str, e))?;
        let cutoff = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid date"))?
            .and_utc();
        repo.find_since(cutoff)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    } else {
        let cutoff = Utc::now() - chrono::Duration::days(90);
        repo.find_since(cutoff)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
    };
    if items.is_empty() {
        println!(
            "{}",
            crate::lang::t!(
                "No observation data. Run `refine ingest-sessions` first.",
                "暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            )
        );
        return Ok(());
    }
    let obs_count = items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
        .count();
    if obs_count == 0 {
        println!(
            "{}",
            crate::lang::t!(
                "No observation data in the time window. Run `refine ingest-sessions` first.",
                "当前时间窗口内无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            )
        );
        return Ok(());
    }
    let cluster = cluster_observations(&items);
    let config = crate::config::load();
    let mut result = compute(&cluster, &config.targets);

    // Try personal baseline: load history BEFORE persisting current score
    let history = load_recent_scores(365)?;
    let baseline = compute_personal_baseline(&history);
    let using_personal = baseline.is_some();

    if let Some(ref bl) = baseline {
        apply_personal_baseline(&mut result, bl);
    }

    persist_score(&result)?;
    print_score(&result, using_personal);

    // Data time range
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
            crate::lang::t!("Data range:", "数据范围:"),
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

    if let Some(llm) = llm {
        match crate::advice::generate_and_cache(&result, &llm).await {
            Ok(advice) => println!("\n  {} {}", crate::lang::t!("Advice:", "建议:"), advice),
            Err(e) => tracing::debug!("advice generation skipped: {}", e),
        }
    }

    if let Err(e) = write_statusline(&result, db_path) {
        tracing::warn!("failed to write statusline.txt: {}", e);
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
