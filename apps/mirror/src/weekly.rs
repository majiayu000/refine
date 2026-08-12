mod action_card;

use crate::config::ensure_mirror_dir;
use crate::document_save::{save_report_to_document, SaveDocumentOptions};
use crate::lang::t;
use crate::score::{self, LayerScore, ScoreResult, Signal};
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, Utc};
use refine_core::knowledge::{DocumentRepository, ItemRepository};
use refine_core::session::{cluster_observations, ClusterResult};
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

    let this_week = item_repo
        .find_observations_by_event_range(week_ago, now)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let last_week = item_repo
        .find_observations_by_event_range(two_weeks_ago, week_ago)
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
    let this_cluster = cluster_observations(&this_week);
    let this_score = score::compute(&this_cluster, &config.targets);

    let last_score = if !last_week.is_empty() {
        let last_cluster = cluster_observations(&last_week);
        Some(score::compute(&last_cluster, &config.targets))
    } else {
        None
    };

    let report = build_weekly_report(&this_score, last_score.as_ref(), &this_cluster);

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

fn build_weekly_report(
    this: &ScoreResult,
    last: Option<&ScoreResult>,
    cluster: &ClusterResult,
) -> String {
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
        Some(last) => {
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

    if let Some(action_card) = action_card::build_weekly_action_card(this, cluster) {
        lines.push(String::new());
        lines.extend(action_card);
    }

    lines.join("\n")
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
mod tests {
    use super::*;
    use crate::score::{Indicator, Signal};
    use refine_core::knowledge::{Item, ItemId, ItemType, RestoreParams, Tag};
    use refine_core::session::{GlobalStats, ProjectCluster};
    use std::collections::{HashMap, HashSet};

    fn make_weekly_record(seed: usize) -> WeeklyRecord {
        WeeklyRecord {
            week_end: Utc::now() + Duration::weeks(seed as i64),
            scores: [
                LayerSignal {
                    name: "depth".into(),
                    signal: "green".into(),
                },
                LayerSignal {
                    name: "breadth".into(),
                    signal: "yellow".into(),
                },
                LayerSignal {
                    name: "collaboration".into(),
                    signal: "red".into(),
                },
            ],
            suggestions: Vec::new(),
        }
    }

    fn make_item_at(time: DateTime<Utc>, idx: usize) -> Item {
        Item::restore(RestoreParams {
            id: ItemId::new(),
            item_type: ItemType::Observation,
            title: format!("obs-{}", idx),
            summary: format!("test observation {}", idx),
            content: String::new(),
            tags: vec![Tag::new("test").unwrap()],
            source: None,
            document_id: None,
            excerpt: None,
            created_at: time,
            updated_at: time,
        })
        .unwrap()
    }

    fn make_score_result(signals: [Signal; 3]) -> ScoreResult {
        let names = ["depth", "breadth", "collaboration"];
        ScoreResult {
            layers: std::array::from_fn(|i| LayerScore {
                name: names[i].to_string(),
                signal: signals[i],
                indicators: Vec::new(),
            }),
            tension: None,
            timestamp: Utc::now(),
        }
    }

    fn empty_cluster() -> ClusterResult {
        ClusterResult {
            projects: HashMap::new(),
            global_stats: GlobalStats {
                total_sessions: 0,
                total_decisions: 0,
                total_bugfixes: 0,
                total_summaries: 0,
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tool_frequency: HashMap::new(),
                project_ranking: Vec::new(),
            },
            untagged_count: 0,
        }
    }

    fn cluster_with_project_evidence() -> ClusterResult {
        let mut projects = HashMap::new();
        projects.insert(
            "codex-tool".to_string(),
            ProjectCluster {
                project_name: "codex-tool".to_string(),
                session_count: 1,
                doc_ids: HashSet::new(),
                summary_excerpts: Vec::new(),
                decision_titles: Vec::new(),
                bugfix_titles: Vec::new(),
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tools: Vec::new(),
                frictions: Vec::new(),
                architectures: Vec::new(),
                knowledge_gained: Vec::new(),
                patterns: Vec::new(),
                progress_items: Vec::new(),
                question_items: vec!["validate Codex session attribution".to_string()],
                code_artifacts: Vec::new(),
            },
        );

        ClusterResult {
            projects,
            global_stats: GlobalStats {
                total_sessions: 1,
                total_decisions: 0,
                total_bugfixes: 0,
                total_summaries: 1,
                cognitive_levels: HashMap::new(),
                collaboration_modes: HashMap::new(),
                tool_frequency: HashMap::new(),
                project_ranking: vec![("codex-tool".to_string(), 1)],
            },
            untagged_count: 0,
        }
    }

    #[test]
    fn test_filter_by_time_range() {
        let now = Utc::now();
        let week_ago = now - Duration::days(7);
        let two_weeks_ago = now - Duration::days(14);

        let mut items = Vec::new();
        for i in 0..2 {
            items.push(make_item_at(now - Duration::days(3), i));
        }
        for i in 0..3 {
            items.push(make_item_at(now - Duration::days(10), 10 + i));
        }
        items.push(make_item_at(now - Duration::days(20), 20));

        let this_week = filter_by_time_range(&items, week_ago, now);
        assert_eq!(this_week.len(), 2);

        let last_week = filter_by_time_range(&items, two_weeks_ago, week_ago);
        assert_eq!(last_week.len(), 3);
    }

    #[test]
    fn test_build_weekly_report_no_suggestions_text() {
        let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let report = build_weekly_report(&score, None, &empty_cluster());
        assert!(!report.contains("建议"), "report must not contain '建议'");
        assert!(
            !report.to_lowercase().contains("suggestion"),
            "report must not contain 'suggestion'"
        );
    }

    #[test]
    fn test_build_weekly_report_contains_three_axes() {
        let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let report = build_weekly_report(&score, None, &empty_cluster());
        assert!(report.contains("depth") || report.contains("深度") || report.contains("Depth"));
        assert!(
            report.contains("breadth") || report.contains("广度") || report.contains("Breadth")
        );
        assert!(
            report.contains("collaboration")
                || report.contains("协作")
                || report.contains("Collab")
        );
    }

    #[test]
    fn test_build_weekly_report_no_prior_data() {
        let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let report = build_weekly_report(&score, None, &empty_cluster());
        assert!(
            report.contains("No prior week data") || report.contains("无上周数据"),
            "should indicate absence of prior data"
        );
    }

    #[test]
    fn test_build_weekly_report_with_prior_data_shows_delta() {
        let this_score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let last_score = make_score_result([Signal::Yellow, Signal::Red, Signal::Green]);
        let report = build_weekly_report(&this_score, Some(&last_score), &empty_cluster());
        assert!(
            report.contains('↑') || report.contains('↓') || report.contains('='),
            "report with prior data must show trend arrows"
        );
    }

    #[test]
    fn test_build_weekly_report_with_indicators() {
        let score = ScoreResult {
            layers: std::array::from_fn(|i| {
                let names = ["depth", "breadth", "collaboration"];
                LayerScore {
                    name: names[i].to_string(),
                    signal: Signal::Green,
                    indicators: vec![Indicator {
                        name: "dreyfus".into(),
                        actual: 4.2,
                        target: ">3.5".into(),
                        signal: Signal::Green,
                    }],
                }
            }),
            tension: Some("test tension".into()),
            timestamp: Utc::now(),
        };
        let report = build_weekly_report(&score, None, &empty_cluster());
        assert!(
            report.contains("4.2"),
            "indicator values must appear in report"
        );
        assert!(report.contains("tension") || report.contains("张力"));
    }

    #[test]
    fn test_build_weekly_report_includes_action_card_from_same_cluster() {
        let score = ScoreResult {
            layers: [
                LayerScore {
                    name: "depth".into(),
                    signal: Signal::Green,
                    indicators: Vec::new(),
                },
                LayerScore {
                    name: "breadth".into(),
                    signal: Signal::Red,
                    indicators: vec![
                        Indicator {
                            name: "exploration".into(),
                            actual: 5.2,
                            target: ">15%".into(),
                            signal: Signal::Red,
                        },
                        Indicator {
                            name: "deep_invest".into(),
                            actual: 19.0,
                            target: "15-30%".into(),
                            signal: Signal::Yellow,
                        },
                        Indicator {
                            name: "fragmentation".into(),
                            actual: 26.0,
                            target: "<20%".into(),
                            signal: Signal::Red,
                        },
                    ],
                },
                LayerScore {
                    name: "collaboration".into(),
                    signal: Signal::Green,
                    indicators: Vec::new(),
                },
            ],
            tension: None,
            timestamp: Utc::now(),
        };

        let report = build_weekly_report(&score, None, &cluster_with_project_evidence());

        assert!(report.contains("Weekly Action Card"));
        assert!(report.contains("codex-tool"));
        assert!(report.contains("validate Codex session attribution"));
    }

    #[test]
    fn test_signal_delta_arrows() {
        assert_eq!(signal_delta(Signal::Green, Signal::Yellow), "↑");
        assert_eq!(signal_delta(Signal::Yellow, Signal::Green), "↓");
        assert_eq!(signal_delta(Signal::Green, Signal::Green), "=");
        assert_eq!(signal_delta(Signal::Red, Signal::Red), "=");
        assert_eq!(signal_delta(Signal::Green, Signal::Red), "↑");
        assert_eq!(signal_delta(Signal::Red, Signal::Green), "↓");
    }

    #[test]
    fn test_load_last_weekly_record_reports_invalid_jsonl_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");
        std::fs::write(&path, "{\"week_end\":\n").unwrap();

        let err = load_last_weekly_record_from_path(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("line 1"));
        assert!(msg.contains("weekly history"));
    }

    #[test]
    fn test_load_last_weekly_record_returns_last_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");

        let first = WeeklyRecord {
            week_end: Utc::now() - Duration::days(7),
            scores: [
                LayerSignal {
                    name: "depth".into(),
                    signal: "green".into(),
                },
                LayerSignal {
                    name: "breadth".into(),
                    signal: "yellow".into(),
                },
                LayerSignal {
                    name: "collaboration".into(),
                    signal: "red".into(),
                },
            ],
            suggestions: Vec::new(),
        };
        let second = WeeklyRecord {
            week_end: Utc::now(),
            scores: [
                LayerSignal {
                    name: "depth".into(),
                    signal: "yellow".into(),
                },
                LayerSignal {
                    name: "breadth".into(),
                    signal: "yellow".into(),
                },
                LayerSignal {
                    name: "collaboration".into(),
                    signal: "yellow".into(),
                },
            ],
            suggestions: Vec::new(),
        };
        let first_line = serde_json::to_string(&first).unwrap();
        let second_line = serde_json::to_string(&second).unwrap();
        std::fs::write(&path, format!("{}\n{}\n", first_line, second_line)).unwrap();

        let last = load_last_weekly_record_from_path(&path).unwrap();
        assert!(last.is_some());
        // Second record has all-yellow signals; verify that record was returned
        assert_eq!(last.unwrap().scores[0].signal, "yellow");
    }

    #[test]
    fn test_load_last_weekly_record_accepts_legacy_missing_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");
        let legacy_line = format!(
            "{{\"week_end\":\"{}\",\"scores\":[{{\"name\":\"depth\",\"signal\":\"green\"}},{{\"name\":\"breadth\",\"signal\":\"yellow\"}},{{\"name\":\"collaboration\",\"signal\":\"red\"}}]}}",
            Utc::now().to_rfc3339()
        );
        std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

        let record = load_last_weekly_record_from_path(&path)
            .unwrap()
            .expect("expected record from legacy line");
        assert!(record.suggestions.is_empty());
        assert_eq!(record.scores[0].name, "depth");
    }

    #[test]
    fn test_load_last_weekly_record_accepts_legacy_with_suggestions() {
        // Old JSONL records that have a "suggestions" field must still deserialize.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");
        let legacy_line = format!(
            "{{\"week_end\":\"{}\",\"scores\":[{{\"name\":\"depth\",\"signal\":\"green\"}},{{\"name\":\"breadth\",\"signal\":\"yellow\"}},{{\"name\":\"collaboration\",\"signal\":\"red\"}}],\"suggestions\":[\"do more reviews\"]}}",
            Utc::now().to_rfc3339()
        );
        std::fs::write(&path, format!("{}\n", legacy_line)).unwrap();

        let record = load_last_weekly_record_from_path(&path)
            .unwrap()
            .expect("expected record from legacy line with suggestions");
        assert_eq!(record.scores[0].name, "depth");
        assert_eq!(record.suggestions, vec!["do more reviews"]);
    }

    #[test]
    fn test_save_weekly_record_caps_history_at_52() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");

        for i in 0..53 {
            let record = make_weekly_record(i);
            persist_weekly_record_to_path(&path, &record).unwrap();
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert_eq!(lines.len(), 52, "history must be capped at 52 records");

        let records: Vec<WeeklyRecord> = lines
            .into_iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // All records use signal "green" for depth in make_weekly_record
        assert_eq!(records.first().unwrap().scores[0].signal, "green");
        assert_eq!(records.last().unwrap().scores[0].signal, "green");
    }

    #[test]
    fn test_new_records_do_not_serialize_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weekly-history.jsonl");
        let record = make_weekly_record(0);
        persist_weekly_record_to_path(&path, &record).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            !content.contains("suggestions"),
            "new records must not serialize the suggestions field"
        );
    }

    #[test]
    fn test_save_last_weekly_md_roundtrip() -> Result<()> {
        let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!(e))?;
        let path = dir.path().join("last-weekly.md");
        let report = "# Weekly Report\n\n- Focus on testing\n- Write more docs\n";
        save_last_weekly_md_to_path(report, &path)?;
        let content = std::fs::read_to_string(&path)?;
        assert_eq!(content, report);
        Ok(())
    }

    #[test]
    fn test_weekly_sentinel_written_with_nonempty_content() -> Result<()> {
        let score = make_score_result([Signal::Green, Signal::Yellow, Signal::Red]);
        let report = build_weekly_report(&score, None, &empty_cluster());

        let dir = tempfile::tempdir().map_err(|e| anyhow::anyhow!(e))?;
        let path = dir.path().join("last-weekly.md");
        save_last_weekly_md_to_path(&report, &path)?;
        let content = std::fs::read_to_string(&path)?;
        assert!(
            !content.trim().is_empty(),
            "sentinel file must not be empty"
        );
        Ok(())
    }
}
