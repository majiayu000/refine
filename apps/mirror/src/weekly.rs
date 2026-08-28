mod action_card;

use crate::config::ensure_mirror_dir;
use crate::document_save::{save_report_to_document, SaveDocumentOptions};
use crate::lang::t;
use crate::score::{self, LayerScore, ScoreResult, Signal};
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, Utc};
use refine_core::knowledge::{DocumentRepository, ItemRepository};
use refine_core::session::{
    cluster_observations_with_resolver, format_data_quality_stats, ClusterResult, DataQualityStats,
    ProjectIdentityResolver,
};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

#[cfg(test)]
use std::io::BufRead;

const WEEKLY_HISTORY_LIMIT: usize = 52;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklyRecord {
    pub week_end: DateTime<Utc>,
    pub scores: [LayerSignal; 3],
    // Retained for backward-compat deserialization of old JSONL records that include
    // this field. Not written in new records (skip_serializing) and not used in
    // production logic (allow(dead_code)).
    #[allow(dead_code)]
    #[serde(default, skip_serializing)]
    pub suggestions: Vec<String>,
}

impl Default for WeeklyRecord {
    fn default() -> Self {
        Self {
            week_end: DateTime::<Utc>::UNIX_EPOCH,
            scores: [
                LayerSignal {
                    name: "depth".to_string(),
                    signal: "yellow".to_string(),
                },
                LayerSignal {
                    name: "breadth".to_string(),
                    signal: "yellow".to_string(),
                },
                LayerSignal {
                    name: "collaboration".to_string(),
                    signal: "yellow".to_string(),
                },
            ],
            suggestions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSignal {
    pub name: String,
    pub signal: String,
}

pub async fn handle_weekly(
    item_repo: Arc<dyn ItemRepository>,
    doc_repo: Arc<dyn DocumentRepository>,
) -> Result<()> {
    let now = Utc::now();
    let week_ago = now - Duration::days(7);
    let two_weeks_ago = now - Duration::days(14);
    let ninety_days_ago = now - Duration::days(crate::advice::LONG_TERM_WINDOW_DAYS);

    let this_week = item_repo
        .find_observations_by_event_range(week_ago, now)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let last_week = item_repo
        .find_observations_by_event_range(two_weeks_ago, week_ago)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let long_term_items = item_repo
        .find_observations_by_event_range(ninety_days_ago, now)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if this_week.is_empty() {
        println!(
            "{}",
            t!(
                "No observations this week.",
                "本周暂无观测数据，无法生成周报。"
            )
        );
        return Ok(());
    }

    println!(
        "{}\n",
        t!(
            format!(
                "This week {} / Last week {} observations",
                this_week.len(),
                last_week.len()
            ),
            format!(
                "本周 {} 条 / 上周 {} 条观测数据",
                this_week.len(),
                last_week.len()
            )
        )
    );

    let config = crate::config::load();
    let resolver = ProjectIdentityResolver::from_observation_windows(&[
        long_term_items.as_slice(),
        this_week.as_slice(),
        last_week.as_slice(),
    ]);
    let this_cluster = cluster_observations_with_resolver(&this_week, &resolver);
    if this_cluster.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "No eligible linked interactive observations this week (input {}, detached {}, mode-excluded {}); refusing to emit scores",
            this_cluster.data_quality.input_observations,
            this_cluster.data_quality.detached_observations,
            this_cluster.data_quality.mode_excluded_observations,
        );
    }
    let this_score = score::compute(&this_cluster, &config.targets);
    let long_term_cluster = cluster_observations_with_resolver(&long_term_items, &resolver);
    if long_term_cluster.data_quality.eligible_observations == 0 {
        anyhow::bail!(
            "No eligible linked interactive observations in the rolling-90-day portfolio window; refusing to generate portfolio advice"
        );
    }
    let long_term_score = score::compute(&long_term_cluster, &config.targets);

    let last_cluster = if !last_week.is_empty() {
        Some(cluster_observations_with_resolver(&last_week, &resolver))
    } else {
        None
    };
    let last_score = last_cluster
        .as_ref()
        .map(|cluster| score::compute(cluster, &config.targets));
    let last_comparison = last_score
        .as_ref()
        .zip(last_cluster.as_ref())
        .map(|(score, cluster)| (score, &cluster.data_quality));

    let report = build_weekly_report_with_portfolio(
        &this_score,
        last_comparison,
        &this_cluster,
        &long_term_score,
        &long_term_cluster,
    )?;

    println!("{}", report);

    save_weekly_record(&this_score)?;
    let doc_id = save_report_to_document(
        &doc_repo,
        &report,
        SaveDocumentOptions {
            source: "mirror-weekly",
            title_prefix: "Mirror Weekly",
            url_scheme: "mirror-weekly",
            save_error_context: "Failed to save weekly report",
        },
    )
    .await?;
    println!(
        "\n{}",
        t!(
            format!("Weekly report saved (ID: {})", doc_id),
            format!("周报已保存 (ID: {})", doc_id)
        )
    );

    save_last_weekly_md(&report)
        .context("weekly report saved, but failed to write the MOTD sentinel")?;

    Ok(())
}

#[cfg(test)]
fn filter_by_time_range(
    items: &[refine_core::knowledge::Item],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Vec<refine_core::knowledge::Item> {
    items
        .iter()
        .filter(|item| {
            let t = item.created_at();
            t >= from && t < to
        })
        .cloned()
        .collect()
}

/// Format the indicator sub-metrics for a layer as a comma-separated list.
fn layer_indicators(layer: &LayerScore) -> String {
    layer
        .indicators
        .iter()
        .map(|i| format!("{}={:.1}", score::indicator_display(&i.name), i.actual))
        .collect::<Vec<_>>()
        .join(", ")
}

fn signal_rank(signal: Signal) -> i8 {
    match signal {
        Signal::Green => 2,
        Signal::Yellow => 1,
        Signal::Red => 0,
    }
}

fn signal_delta(current: Signal, previous: Signal) -> &'static str {
    match signal_rank(current) - signal_rank(previous) {
        d if d > 0 => "↑",
        d if d < 0 => "↓",
        _ => "=",
    }
}

fn build_weekly_report_with_portfolio(
    this: &ScoreResult,
    last: Option<(&ScoreResult, &DataQualityStats)>,
    recent_cluster: &ClusterResult,
    long_term: &ScoreResult,
    long_term_cluster: &ClusterResult,
) -> Result<String> {
    let now = Utc::now();
    let week_num = now.iso_week().week();
    let year = now.iso_week().year();

    let mut lines = Vec::new();
    lines.push(format!("# Mirror Weekly — {}-W{:02}", year, week_num));
    lines.push(String::new());
    lines.push(format!(
        "> {}",
        t!(
            "Metrics-derived report — for coaching run `refine cognitive-portrait`",
            "指标驱动报告 — 教练分析请运行 `refine cognitive-portrait`"
        )
    ));
    lines.push(format!(
        "> Cohort: linked observations excluding unattended/subagent documents · Data quality: {}",
        format_data_quality_stats(&recent_cluster.data_quality)
    ));
    lines.push(format!(
        "> {}",
        t!(
            "Window: rolling 7 days (event time) · signals: absolute targets",
            "窗口: 滚动 7 天(事件时间) · 信号灯: 绝对目标"
        )
    ));

    // Section A: This-week 3-axis signal table
    lines.push(String::new());
    lines.push(format!("## {}", t!("This Week Signals", "本周信号灯")));
    lines.push(format!(
        "| {} | {} | {} |",
        t!("Dimension", "维度"),
        t!("Signal", "信号"),
        t!("Indicators", "子指标")
    ));
    lines.push("| --- | --- | --- |".to_string());
    for layer in &this.layers {
        lines.push(format!(
            "| {} | {} {} | {} |",
            score::layer_display(&layer.name),
            layer.signal.emoji(),
            layer.signal.as_str(),
            layer_indicators(layer)
        ));
    }
    if let Some(tension) = &this.tension {
        lines.push(String::new());
        lines.push(format!("{}: {}", t!("Tension", "张力"), tension));
    }

    // Section B: Signal delta vs last week
    lines.push(String::new());
    lines.push(format!(
        "## {}",
        t!("Signal Delta vs Last Week", "vs 上周信号变化")
    ));
    match last {
        _ if recent_cluster.data_quality.is_degraded() => {
            lines.push(
                t!(
                    "DEGRADED data quality: detached observations were excluded; week-over-week trend is suppressed.",
                    "数据质量为 DEGRADED：脱链观测已排除，本周不输出环比趋势。"
                )
                .to_string(),
            );
        }
        Some((_, quality)) if quality.is_degraded() => {
            lines.push(
                t!(
                    "Prior-week data quality is DEGRADED; week-over-week trend is suppressed.",
                    "上周数据质量为 DEGRADED，本周不输出环比趋势。"
                )
                .to_string(),
            );
        }
        Some((_, quality)) if quality.eligible_observations == 0 => {
            lines.push(
                t!(
                    "No eligible linked observations in the prior week; week-over-week trend is unavailable.",
                    "上周没有合格的已关联观测，无法计算环比趋势。"
                )
                .to_string(),
            );
        }
        Some((last, _)) => {
            lines.push(format!(
                "| {} | {} | {} | {} |",
                t!("Dimension", "维度"),
                t!("Last Week", "上周"),
                t!("This Week", "本周"),
                t!("Trend", "趋势")
            ));
            lines.push("| --- | --- | --- | --- |".to_string());
            for (cur_layer, last_layer) in this.layers.iter().zip(last.layers.iter()) {
                lines.push(format!(
                    "| {} | {} {} | {} {} | {} |",
                    score::layer_display(&cur_layer.name),
                    last_layer.signal.emoji(),
                    last_layer.signal.as_str(),
                    cur_layer.signal.emoji(),
                    cur_layer.signal.as_str(),
                    signal_delta(cur_layer.signal, last_layer.signal)
                ));
            }
        }
        None => {
            lines.push(t!("No prior week data.", "无上周数据。").to_string());
        }
    }

    if let Some(action_card) =
        action_card::build_weekly_action_card(long_term, this, long_term_cluster, recent_cluster)?
    {
        lines.push(String::new());
        lines.extend(action_card);
    }

    Ok(lines.join("\n"))
}

#[cfg(test)]
fn build_weekly_report(
    this: &ScoreResult,
    last: Option<(&ScoreResult, &DataQualityStats)>,
    cluster: &ClusterResult,
) -> String {
    let mut portfolio = this.clone();
    if let Some(breadth) = portfolio
        .layers
        .iter_mut()
        .find(|layer| layer.name == "breadth")
    {
        for name in ["exploration", "fragmentation"] {
            if !breadth
                .indicators
                .iter()
                .any(|indicator| indicator.name == name)
            {
                breadth.indicators.push(crate::score::Indicator {
                    name: name.to_string(),
                    actual: if name == "exploration" { 20.0 } else { 5.0 },
                    target: String::new(),
                    signal: Signal::Green,
                });
            }
        }
    }
    build_weekly_report_with_portfolio(&portfolio, last, cluster, &portfolio, cluster)
        .expect("weekly report fixture must include valid portfolio inputs")
}

#[cfg(test)]
fn load_last_weekly_record_from_path(path: &Path) -> Result<Option<WeeklyRecord>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to open weekly history {}: {}",
                path.display(),
                e
            ));
        }
    };
    let reader = std::io::BufReader::new(file);
    let mut last = None;
    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read line {} from weekly history {}",
                line_no,
                path.display()
            )
        })?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = serde_json::from_str::<WeeklyRecord>(line).with_context(|| {
            format!(
                "failed to parse JSON on line {} in weekly history {}",
                line_no,
                path.display()
            )
        })?;
        last = Some(record);
    }
    Ok(last)
}

fn save_last_weekly_md(report: &str) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    save_last_weekly_md_to_path(report, &dir.join("last-weekly.md"))
}

fn save_last_weekly_md_to_path(report: &str, path: &Path) -> Result<()> {
    std::fs::write(path, report)
        .with_context(|| format!("failed to write last weekly report to {}", path.display()))
}

fn save_weekly_record(score: &ScoreResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("weekly-history.jsonl");
    let record = WeeklyRecord {
        week_end: Utc::now(),
        scores: [
            LayerSignal {
                name: score.layers[0].name.clone(),
                signal: score.layers[0].signal.as_str().to_string(),
            },
            LayerSignal {
                name: score.layers[1].name.clone(),
                signal: score.layers[1].signal.as_str().to_string(),
            },
            LayerSignal {
                name: score.layers[2].name.clone(),
                signal: score.layers[2].signal.as_str().to_string(),
            },
        ],
        suggestions: Vec::new(),
    };
    persist_weekly_record_to_path(&path, &record)
}

fn persist_weekly_record_to_path(path: &Path, record: &WeeklyRecord) -> Result<()> {
    let mut lines: Vec<String> = match std::fs::read_to_string(path) {
        Ok(content) => content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read weekly history {}: {}",
                path.display(),
                e
            ));
        }
    };

    lines.push(serde_json::to_string(record)?);
    if lines.len() > WEEKLY_HISTORY_LIMIT {
        let trim = lines.len() - WEEKLY_HISTORY_LIMIT;
        lines.drain(0..trim);
    }

    write_weekly_history_lines_atomically(path, &lines)
}

fn write_weekly_history_lines_atomically(path: &Path, lines: &[String]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "weekly history path has no parent directory: {}",
            path.display()
        )
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("weekly-history.jsonl");
    let nonce = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let temp_path = parent.join(format!(
        ".{}.tmp-{}-{}",
        file_name,
        std::process::id(),
        nonce
    ));

    let write_result = (|| -> Result<()> {
        let mut temp_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .with_context(|| {
                format!(
                    "failed to create temp weekly history file {}",
                    temp_path.display()
                )
            })?;

        for line in lines {
            writeln!(temp_file, "{}", line)?;
        }
        temp_file.sync_all().with_context(|| {
            format!(
                "failed to fsync temp weekly history file {}",
                temp_path.display()
            )
        })?;

        std::fs::rename(&temp_path, path).with_context(|| {
            format!(
                "failed to atomically replace weekly history {}",
                path.display()
            )
        })?;

        if let Ok(dir_file) = std::fs::File::open(parent) {
            let _ = dir_file.sync_all();
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }

    write_result
}

#[cfg(test)]
mod tests;
