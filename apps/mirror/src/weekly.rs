use crate::config::{ensure_mirror_dir, mirror_dir};
use crate::score::{self, LayerScore, ScoreResult, Signal};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentRepository, Item, ItemRepository, ItemType};
use refine_core::session::cluster_observations;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::sync::Arc;

const MAX_RETRIES: usize = 5;
const RETRY_BASE_DELAY_SECS: u64 = 10;

const SYSTEM_PROMPT: &str = "你是认知成长分析师。基于开发者本周 vs 上周的编程会话数据变化，\
生成差量报告。使用中文。用第二人称。";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyRecord {
    pub week_end: DateTime<Utc>,
    pub scores: [LayerSignal; 3],
    pub suggestions: Vec<String>,
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
        println!("本周暂无观测数据，无法生成周报。");
        return Ok(());
    }

    println!(
        "本周 {} 条 / 上周 {} 条观测数据\n",
        this_week.len(),
        last_week.len()
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

    let prev_record = load_last_weekly_record();
    let prompt = build_weekly_prompt(&this_score, last_score.as_ref(), prev_record.as_ref());
    let report = llm_with_retry(&llm, &prompt, SYSTEM_PROMPT).await?;

    println!("{}", report);

    save_weekly_record(&this_score)?;
    save_to_document(&doc_repo, &report).await?;

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
        .map(|i| format!("{}={:.1}", i.name, i.actual))
        .collect();
    format!(
        "{}[{}]: {}",
        layer.name,
        signal_str(&layer.signal),
        indicators.join(", ")
    )
}

fn signal_str(s: &Signal) -> &'static str {
    match s {
        Signal::Green => "green",
        Signal::Yellow => "yellow",
        Signal::Red => "red",
    }
}

pub fn build_weekly_prompt(
    this: &ScoreResult,
    last: Option<&ScoreResult>,
    prev_record: Option<&WeeklyRecord>,
) -> String {
    let mut parts = Vec::new();
    parts.push("## 本周信号灯".to_string());
    for layer in &this.layers {
        parts.push(format!("- {}", format_layer(layer)));
    }
    if let Some(tension) = &this.tension {
        parts.push(format!("\n张力: {}", tension));
    }

    if let Some(last) = last {
        parts.push("\n## 上周信号灯".to_string());
        for layer in &last.layers {
            parts.push(format!("- {}", format_layer(layer)));
        }
    }

    if let Some(record) = prev_record {
        if !record.suggestions.is_empty() {
            parts.push("\n## 上周建议".to_string());
            for s in &record.suggestions {
                parts.push(format!("- {}", s));
            }
        }
    }

    parts.push("\n## 要求".to_string());
    parts.push("1. 各维度变化分析（对比本周 vs 上周信号灯和子指标）".to_string());
    parts.push("2. 上周建议执行情况评估（如有上周建议）".to_string());
    parts.push("3. 下周 1-2 条具体建议".to_string());
    parts.join("\n")
}

fn load_last_weekly_record() -> Option<WeeklyRecord> {
    let path = mirror_dir().join("weekly-history.jsonl");
    let file = std::fs::File::open(&path).ok()?;
    let reader = std::io::BufReader::new(file);
    reader
        .lines()
        .filter_map(|l| l.ok())
        .filter_map(|l| serde_json::from_str::<WeeklyRecord>(&l).ok())
        .last()
}

fn save_weekly_record(score: &ScoreResult) -> Result<()> {
    let dir = ensure_mirror_dir()?;
    let path = dir.join("weekly-history.jsonl");
    let record = WeeklyRecord {
        week_end: Utc::now(),
        scores: [
            LayerSignal {
                name: score.layers[0].name.clone(),
                signal: signal_str(&score.layers[0].signal).to_string(),
            },
            LayerSignal {
                name: score.layers[1].name.clone(),
                signal: signal_str(&score.layers[1].signal).to_string(),
            },
            LayerSignal {
                name: score.layers[2].name.clone(),
                signal: signal_str(&score.layers[2].signal).to_string(),
            },
        ],
        suggestions: Vec::new(),
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(&record)?;
    writeln!(file, "{}", line)?;
    Ok(())
}

async fn save_to_document(doc_repo: &Arc<dyn DocumentRepository>, report: &str) -> Result<()> {
    let mut doc = Document::new("mirror-weekly", report);
    let title = format!("Mirror Weekly {}", doc.created_at().format("%Y-%m-%d"));
    doc.set_title(&title);
    doc.set_url(&format!(
        "mirror-weekly://{}",
        doc.created_at().to_rfc3339()
    ));
    doc_repo.save(&doc).await.context("保存周报失败")?;
    println!("\n周报已保存 (ID: {})", doc.id());
    Ok(())
}

async fn llm_with_retry(
    client: &Arc<dyn LlmClient>,
    prompt: &str,
    system: &str,
) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..MAX_RETRIES {
        match client.complete(prompt, Some(system)).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_err = e.to_string();
                let is_retryable = last_err.contains("cooldown")
                    || last_err.contains("service_busy")
                    || last_err.contains("rate")
                    || last_err.contains("429")
                    || last_err.contains("Upstream")
                    || last_err.contains("timeout")
                    || last_err.contains("empty response");
                if !is_retryable || attempt == MAX_RETRIES - 1 {
                    break;
                }
                let delay = RETRY_BASE_DELAY_SECS * (1 << attempt);
                eprintln!(
                    "  重试 ({}/{}) 等待 {}s...",
                    attempt + 1,
                    MAX_RETRIES,
                    delay
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
        }
    }
    Err(anyhow::anyhow!(
        "LLM 调用失败 ({}次重试): {}",
        MAX_RETRIES,
        last_err
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use refine_core::knowledge::{ItemId, RestoreParams, Tag};

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
                    name: "认知深度".into(),
                    signal: Signal::Green,
                    indicators: Vec::new(),
                },
                LayerScore {
                    name: "战略广度".into(),
                    signal: Signal::Yellow,
                    indicators: Vec::new(),
                },
                LayerScore {
                    name: "协作效能".into(),
                    signal: Signal::Red,
                    indicators: Vec::new(),
                },
            ],
            tension: Some("test tension".into()),
            timestamp: Utc::now(),
        };

        let prompt = build_weekly_prompt(&score, None, None);
        assert!(prompt.contains("本周信号灯"));
        assert!(prompt.contains("认知深度"));
        assert!(prompt.contains("green"));
        assert!(prompt.contains("张力"));
        assert!(prompt.contains("各维度变化分析"));
    }
}
