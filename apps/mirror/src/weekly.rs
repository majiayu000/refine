use crate::config::{ensure_mirror_dir, mirror_dir};
use crate::document_save::{save_report_to_document, SaveDocumentOptions};
use crate::lang::t;
use crate::llm_retry::llm_with_retry;
use crate::score::{self, LayerScore, ScoreResult};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{DocumentRepository, Item, ItemRepository, ItemType};
use refine_core::session::cluster_observations;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;

const WEEKLY_HISTORY_LIMIT: usize = 52;

fn system_prompt() -> &'static str {
    t!(
        "You are a cognitive growth analyst. Based on the developer's weekly session data changes, \
         generate a delta report. Use English. Address the developer as 'you'.",
        "你是认知成长分析师。基于开发者本周 vs 上周的编程会话数据变化，\
         生成差量报告。使用中文。用第二人称。"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklyRecord {
    pub week_end: DateTime<Utc>,
    pub scores: [LayerSignal; 3],
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
    llm: Arc<dyn LlmClient>,
) -> Result<()> {
    let now = Utc::now();
    let week_ago = now - Duration::days(7);
    let two_weeks_ago = now - Duration::days(14);

    let observations = item_repo
        .find_by_type(ItemType::Observation)
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let this_week = filter_by_time_range(&observations, week_ago, now);
    let last_week = filter_by_time_range(&observations, two_weeks_ago, week_ago);

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

    let prev_record = match load_last_weekly_record() {
        Ok(record) => record,
        Err(e) => {
            tracing::warn!("failed to load weekly history: {}", e);
            None
        }
    };
    let prompt = build_weekly_prompt(&this_score, last_score.as_ref(), prev_record.as_ref());
    let report = llm_with_retry(&llm, &prompt, system_prompt()).await?;

    println!("{}", report);

    let suggestions = extract_suggestions(&report);
    save_weekly_record(&this_score, suggestions)?;
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

    Ok(())
}

fn filter_by_time_range(items: &[Item], from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Item> {
    items
        .iter()
        .filter(|item| {
            let t = item.created_at();
            t >= from && t < to
        })
        .cloned()
        .collect()
}

fn format_layer(layer: &LayerScore) -> String {
    let indicators: Vec<String> = layer
        .indicators
        .iter()
        .map(|i| format!("{}={:.1}", score::indicator_display(&i.name), i.actual))
        .collect();
    format!(
        "{}[{}]: {}",
        score::layer_display(&layer.name),
        layer.signal.as_str(),
        indicators.join(", ")
    )
}

pub fn build_weekly_prompt(
    this: &ScoreResult,
    last: Option<&ScoreResult>,
    prev_record: Option<&WeeklyRecord>,
) -> String {
    let mut parts = Vec::new();
    parts.push(t!("## This Week Signal Lights", "## 本周信号灯").to_string());
    for layer in &this.layers {
        parts.push(format!("- {}", format_layer(layer)));
    }
    if let Some(tension) = &this.tension {
        parts.push(format!("\n{}: {}", t!("Tension", "张力"), tension));
    }

    if let Some(last) = last {
        parts.push(format!(
            "\n{}",
            t!("## Last Week Signal Lights", "## 上周信号灯")
        ));
        for layer in &last.layers {
            parts.push(format!("- {}", format_layer(layer)));
        }
    }

    if let Some(record) = prev_record {
        if !record.suggestions.is_empty() {
            parts.push(format!(
                "\n{}",
                t!("## Last Week Suggestions", "## 上周建议")
            ));
            for s in &record.suggestions {
                parts.push(format!("- {}", s));
            }
        }
    }

    parts.push(format!("\n{}", t!("## Requirements", "## 要求")));
    parts.push(
        t!(
            "1. Dimension change analysis (compare this vs last week signals and indicators)",
            "1. 各维度变化分析（对比本周 vs 上周信号灯和子指标）"
        )
        .to_string(),
    );
    parts.push(
        t!(
            "2. Evaluate last week's suggestion execution (if applicable)",
            "2. 上周建议执行情况评估（如有上周建议）"
        )
        .to_string(),
    );
    parts.push(
        t!(
            "3. 1-2 specific suggestions for next week",
            "3. 下周 1-2 条具体建议"
        )
        .to_string(),
    );
    parts.join("\n")
}

fn load_last_weekly_record() -> Result<Option<WeeklyRecord>> {
    let path = mirror_dir().join("weekly-history.jsonl");
    load_last_weekly_record_from_path(&path)
}

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

fn extract_suggestions(report: &str) -> Vec<String> {
    let lines: Vec<&str> = report.lines().collect();
    let mut suggestions = Vec::new();
    let mut in_suggestion_section = false;

    for line in &lines {
        let trimmed = line.trim();
        let lowered = trimmed.to_lowercase();
        // Detect suggestion section headers (Chinese and English)
        let looks_like_suggestion_header = trimmed.contains("建议")
            || lowered.contains("suggestion")
            || lowered.contains("next week")
            || trimmed.contains("下周");
        if looks_like_suggestion_header && (trimmed.starts_with('#') || trimmed.starts_with("**")) {
            in_suggestion_section = true;
            continue;
        }
        // New section header ends suggestion section
        if in_suggestion_section
            && (trimmed.starts_with('#') || trimmed.starts_with("**"))
            && !trimmed.contains("建议")
            && !lowered.contains("suggestion")
        {
            in_suggestion_section = false;
        }
        // Extract numbered or bulleted list items in suggestion section
        if in_suggestion_section && !trimmed.is_empty() {
            let is_list_item = trimmed.starts_with('-')
                || trimmed.starts_with('*')
                || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit());
            if is_list_item {
                // Strip leading bullet/number markers
                let content = trimmed
                    .trim_start_matches(|c: char| {
                        c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ')'
                    })
                    .trim();
                if !content.is_empty() {
                    suggestions.push(content.to_string());
                }
            }
        }
    }
    suggestions
}

fn save_weekly_record(score: &ScoreResult, suggestions: Vec<String>) -> Result<()> {
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
        suggestions,
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
    use crate::score::Signal;
    use refine_core::knowledge::{ItemId, RestoreParams, Tag};

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
            suggestions: vec![format!("suggestion-{}", seed)],
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
    fn test_build_weekly_prompt() {
        let score = ScoreResult {
            layers: [
                LayerScore {
                    name: "depth".into(),
                    signal: Signal::Green,
                    indicators: Vec::new(),
                },
                LayerScore {
                    name: "breadth".into(),
                    signal: Signal::Yellow,
                    indicators: Vec::new(),
                },
                LayerScore {
                    name: "collaboration".into(),
                    signal: Signal::Red,
                    indicators: Vec::new(),
                },
            ],
            tension: Some("test tension".into()),
            timestamp: Utc::now(),
        };

        let prompt = build_weekly_prompt(&score, None, None);
        assert!(prompt.contains("This Week Signal Lights"));
        assert!(prompt.contains("Depth"));
        assert!(prompt.contains("green"));
        assert!(prompt.contains("Tension"));
        assert!(prompt.contains("Dimension change analysis"));
    }

    #[test]
    fn test_extract_suggestions_chinese() {
        let report = "\
## 各维度变化分析
深度指标从黄灯转绿灯，进步明显。

## 下周建议
1. 每天至少做一次深度 code review
2. 尝试用 Rust 重写核心模块
";
        let suggestions = extract_suggestions(report);
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions[0].contains("code review"));
        assert!(suggestions[1].contains("Rust"));
    }

    #[test]
    fn test_extract_suggestions_english() {
        let report = "\
## Dimension Analysis
Depth improved from yellow to green.

## Suggestions for Next Week
- Focus on writing more tests
- Try pair programming sessions
";
        let suggestions = extract_suggestions(report);
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions[0].contains("tests"));
        assert!(suggestions[1].contains("pair programming"));
    }

    #[test]
    fn test_extract_suggestions_empty_when_no_section() {
        let report = "\
## Analysis
Everything looks good. No changes needed.
";
        let suggestions = extract_suggestions(report);
        assert!(suggestions.is_empty());
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
            suggestions: vec!["first".into()],
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
            suggestions: vec!["second".into()],
        };
        let first_line = serde_json::to_string(&first).unwrap();
        let second_line = serde_json::to_string(&second).unwrap();
        std::fs::write(&path, format!("{}\n{}\n", first_line, second_line)).unwrap();

        let last = load_last_weekly_record_from_path(&path).unwrap();
        assert!(last.is_some());
        assert_eq!(last.unwrap().suggestions, vec!["second".to_string()]);
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
        assert_eq!(lines.len(), 52);

        let records: Vec<WeeklyRecord> = lines
            .into_iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records.first().unwrap().suggestions, vec!["suggestion-1"]);
        assert_eq!(records.last().unwrap().suggestions, vec!["suggestion-52"]);
    }
}
