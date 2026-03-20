//! 认知成长仪表盘

use anyhow::{Context, Result};
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{ItemRepository, ItemType};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

const COGNITIVE_LEVELS: &[&str] = &[
    "expert", "proficient", "competent", "advanced_beginner", "novice",
];

const COLLAB_MODES: &[&str] = &[
    "delegation", "deep_inquiry", "exploration", "review",
    "pair_programming", "teaching", "debugging",
];

const BAR_WIDTH: usize = 10;
const BOX_W: usize = 56;

#[derive(Deserialize)]
struct WeekTracker {
    #[serde(default)]
    total_sessions: usize,
    #[serde(default)]
    exploration_sessions: usize,
    #[serde(default)]
    deep_inquiry_sessions: usize,
}

pub async fn handle_growth(store: Arc<SqliteStore>) -> Result<()> {
    let item_store: &dyn ItemRepository = store.as_ref();
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .context("加载 Observation 失败")?;

    if observations.is_empty() {
        println!("暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。");
        return Ok(());
    }

    let mut session_ids = std::collections::HashSet::new();
    let mut cognitive_counts: HashMap<String, usize> = HashMap::new();
    let mut collab_counts: HashMap<String, usize> = HashMap::new();

    for item in &observations {
        if let Some(doc_id) = item.document_id() {
            session_ids.insert(doc_id.as_str().to_string());
        }
        for tag in item.tags() {
            let t = tag.as_str();
            if is_cognitive(t) {
                *cognitive_counts.entry(t.to_string()).or_default() += 1;
            }
            if is_collab(t) {
                *collab_counts.entry(t.to_string()).or_default() += 1;
            }
        }
    }

    let total_sessions = session_ids.len();
    let total_obs = observations.len();
    let cognitive_total: usize = cognitive_counts.values().sum();
    let collab_total: usize = collab_counts.values().sum();
    let week_data = load_week_tracker();

    // 输出
    println!("{}", border_top());
    println!("{}", row_center("认知成长仪表盘"));
    println!("{}", border_mid());
    println!("{}", row_left(&format!(
        "总会话: {}  总观测: {}", total_sessions, total_obs
    )));
    println!("{}", border_mid());

    println!("{}", row_left("认知水平分布"));
    for &level in COGNITIVE_LEVELS {
        let count = cognitive_counts.get(level).copied().unwrap_or(0);
        println!("{}", row_bar(short_cognitive(level), count, cognitive_total));
    }
    println!("{}", border_mid());

    println!("{}", row_left("协作模式"));
    for &mode in COLLAB_MODES {
        let count = collab_counts.get(mode).copied().unwrap_or(0);
        println!("{}", row_bar(short_collab(mode), count, collab_total));
    }
    println!("{}", border_mid());

    println!("{}", row_left("关键指标"));
    let exploration_rate = pct(collab_counts.get("exploration").copied().unwrap_or(0), collab_total);
    let delegation_rate = pct(collab_counts.get("delegation").copied().unwrap_or(0), collab_total);
    let expert_rate = pct(cognitive_counts.get("expert").copied().unwrap_or(0), cognitive_total);
    println!("{}", row_metric("探索率", exploration_rate, ">15%", exploration_rate > 15.0));
    println!("{}", row_metric("delegation", delegation_rate, "<40%", delegation_rate < 40.0));
    println!("{}", row_metric("expert率", expert_rate, ">15%", expert_rate > 15.0));
    println!("{}", border_mid());

    match week_data {
        Some(wk) => {
            println!("{}", row_left("本周"));
            println!("{}", row_left(&format!(
                "  Sessions: {}  探索: {}  深度: {}",
                wk.total_sessions, wk.exploration_sessions, wk.deep_inquiry_sessions,
            )));
        }
        None => {
            println!("{}", row_left("本周: 无 tracker 数据"));
        }
    }
    println!("{}", border_bot());

    Ok(())
}

fn load_week_tracker() -> Option<WeekTracker> {
    let home = dirs::home_dir()?;
    let path = home.join(".refine").join("growth-tracker.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn is_cognitive(tag: &str) -> bool {
    matches!(tag, "novice" | "advanced_beginner" | "competent" | "proficient" | "expert")
}

fn is_collab(tag: &str) -> bool {
    matches!(tag, "delegation" | "pair_programming" | "review" | "exploration" | "teaching" | "deep_inquiry" | "debugging")
}

fn short_cognitive(s: &str) -> &str {
    match s { "advanced_beginner" => "adv_begin", other => other }
}

fn short_collab(s: &str) -> &str {
    match s { "pair_programming" => "pair_prog", "deep_inquiry" => "deep_inq", other => other }
}

fn pct(count: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { count as f64 / total as f64 * 100.0 }
}

// ── 行渲染（所有 padding 用 saturating_sub 防溢出）──

fn row_bar(label: &str, count: usize, total: usize) -> String {
    let ratio = if total == 0 { 0.0 } else { count as f64 / total as f64 };
    let filled = (ratio * BAR_WIDTH as f64).round() as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled));
    let core = format!("  {:<11} {} {:>5.1}% ({:>3})", label, bar, ratio * 100.0, count);
    padded_row(&core)
}

fn row_metric(label: &str, value: f64, target: &str, ok: bool) -> String {
    let mark = if ok { "✓" } else { "✗" };
    let core = format!("  {:<12} {:>5.1}%  目标: {:<6} {}", label, value, target, mark);
    padded_row(&core)
}

fn row_left(text: &str) -> String {
    padded_row(&format!(" {}", text))
}

fn row_center(text: &str) -> String {
    let tw = display_width(text);
    let inner = BOX_W.saturating_sub(2);
    let left = inner.saturating_sub(tw) / 2;
    let right = inner.saturating_sub(tw).saturating_sub(left);
    format!("║{}{}{}║", " ".repeat(left), text, " ".repeat(right))
}

fn padded_row(content: &str) -> String {
    let cw = display_width(content);
    let inner = BOX_W.saturating_sub(2);
    let padding = inner.saturating_sub(cw);
    format!("║{}{}║", content, " ".repeat(padding))
}

fn border_top() -> String { format!("╔{}╗", "═".repeat(BOX_W.saturating_sub(2))) }
fn border_mid() -> String { format!("╠{}╣", "═".repeat(BOX_W.saturating_sub(2))) }
fn border_bot() -> String { format!("╚{}╝", "═".repeat(BOX_W.saturating_sub(2))) }

fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    matches!(c,
        '\u{2500}'..='\u{259F}' | '\u{2600}'..='\u{27BF}' |
        '\u{4E00}'..='\u{9FFF}' | '\u{3000}'..='\u{303F}' |
        '\u{FF00}'..='\u{FFEF}' | '\u{2E80}'..='\u{2EFF}' |
        '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pct_zero_total_returns_zero() { assert_eq!(pct(5, 0), 0.0); }

    #[test]
    fn pct_normal() { assert!((pct(25, 100) - 25.0).abs() < f64::EPSILON); }

    #[test]
    fn cognitive_and_collab_tags() {
        assert!(is_cognitive("expert"));
        assert!(!is_cognitive("delegation"));
        assert!(is_collab("delegation"));
        assert!(!is_collab("expert"));
    }

    #[test]
    fn padded_row_does_not_panic() {
        // 即使内容超宽也不 panic
        let long = "x".repeat(200);
        let _ = padded_row(&long);
    }
}
