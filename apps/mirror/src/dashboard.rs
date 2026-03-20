use crate::lang::t;
use crate::score::{self, ScoreResult, Signal};
use anyhow::{Context, Result};
use refine_core::knowledge::ItemRepository;
use refine_core::session::cluster_observations;
use std::sync::Arc;

const COGNITIVE: &[(&str, &str)] = &[
    ("expert", "expert"), ("proficient", "proficient"), ("competent", "competent"),
    ("adv_begin", "advanced_beginner"), ("novice", "novice"),
];
const COLLAB: &[(&str, &str)] = &[
    ("delegation", "delegation"), ("deep_inq", "deep_inquiry"),
    ("exploration", "exploration"), ("review", "review"),
    ("pair_prog", "pair_programming"), ("teaching", "teaching"),
    ("debugging", "debugging"),
];
const BAR_W: usize = 10;
const BOX_W: usize = 56;

pub async fn handle_dashboard(repo: Arc<dyn ItemRepository>) -> Result<()> {
    let items = repo.find_all().await.map_err(|e| anyhow::anyhow!("{}", e))?;
    if items.is_empty() {
        println!(
            "{}",
            t!(
                "No observation data. Run `refine ingest-sessions` first.",
                "暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            )
        );
        return Ok(());
    }

    let cluster = cluster_observations(&items);
    let config = crate::config::load();
    let result = score::compute(&cluster, &config.targets);
    score::persist_score(&result).context("Failed to persist score")?;

    let stats = &cluster.global_stats;
    let cog_total: usize = stats.cognitive_levels.values().sum();
    let collab_total: usize = stats.collaboration_modes.values().sum();

    println!("{}", border_top());
    println!(
        "{}",
        row_center(t!("Mirror Cognitive Dashboard", "Mirror 认知仪表盘"))
    );
    println!("{}", border_mid());

    for layer in &result.layers {
        let details: Vec<String> = layer.indicators.iter().map(format_indicator).collect();
        let line = format!(
            " {:<12} {}  {}",
            score::layer_display(&layer.name),
            layer.signal,
            details.join(" | ")
        );
        println!("{}", padded_row(&line));
    }
    println!("{}", border_mid());

    match result.tension {
        Some(ref tension) => println!(
            "{}",
            padded_row(&format!("{}{}", t!(" Tension: ", " 张力: "), tension))
        ),
        None => println!(
            "{}",
            padded_row(t!(
                " Tension: no obvious conflict",
                " 张力: 无明显维度冲突"
            ))
        ),
    }
    println!("{}", border_mid());

    println!(
        "{}",
        padded_row(t!(" Cognitive Level Distribution", " 认知水平分布"))
    );
    for (label, key) in COGNITIVE {
        let n = stats.cognitive_levels.get(*key).copied().unwrap_or(0);
        println!("{}", row_bar(label, n, cog_total));
    }
    println!("{}", border_mid());

    println!("{}", padded_row(t!(" Collaboration Modes", " 协作模式")));
    for (label, key) in COLLAB {
        let n = stats.collaboration_modes.get(*key).copied().unwrap_or(0);
        println!("{}", row_bar(label, n, collab_total));
    }
    println!("{}", border_mid());

    print_trend(&result)?;
    println!("{}", border_bot());
    Ok(())
}

fn format_indicator(ind: &score::Indicator) -> String {
    let name = score::indicator_display(&ind.name);
    match ind.name.as_str() {
        "mode_diversity" => format!("{} {}", name, ind.actual as usize),
        "bug_decision" => format!("{} {:.2}", name, ind.actual),
        "dreyfus" => format!("{} {:.1}", name, ind.actual),
        _ => format!("{} {:.0}%", name, ind.actual),
    }
}

fn print_trend(current: &ScoreResult) -> Result<()> {
    let mut history = score::load_recent_scores(6)?;
    if history.is_empty() {
        history.push(current.clone());
    }
    println!(
        "{}",
        padded_row(t!(" Recent Score Trend", " 最近评分趋势"))
    );
    let entries: Vec<String> = history
        .iter()
        .map(|s| {
            let date = s.timestamp.format("%m-%d");
            let lights: String = s
                .layers
                .iter()
                .map(|l| signal_ansi(l.signal))
                .collect::<Vec<_>>()
                .join("");
            format!("{} {}", date, lights)
        })
        .collect();
    println!("{}", padded_row(&format!("  {}", entries.join("  "))));
    Ok(())
}

fn signal_ansi(s: Signal) -> &'static str {
    match s {
        Signal::Green => "\x1b[32m●\x1b[0m",
        Signal::Yellow => "\x1b[33m●\x1b[0m",
        Signal::Red => "\x1b[31m●\x1b[0m",
    }
}

// ── Box drawing ──

fn row_bar(label: &str, count: usize, total: usize) -> String {
    let ratio = if total == 0 { 0.0 } else { count as f64 / total as f64 };
    let filled = (ratio * BAR_W as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(BAR_W.saturating_sub(filled)),
    );
    padded_row(&format!(
        "  {:<11} {} {:>5.1}% ({:>3})",
        label,
        bar,
        ratio * 100.0,
        count
    ))
}

fn row_center(text: &str) -> String {
    let tw = display_width(text);
    let inner = BOX_W.saturating_sub(2);
    let left = inner.saturating_sub(tw) / 2;
    let right = inner.saturating_sub(tw).saturating_sub(left);
    format!(
        "\u{2551}{}{}{}\u{2551}",
        " ".repeat(left),
        text,
        " ".repeat(right)
    )
}

fn padded_row(content: &str) -> String {
    let cw = display_width(content);
    let inner = BOX_W.saturating_sub(2);
    format!(
        "\u{2551}{}{}\u{2551}",
        content,
        " ".repeat(inner.saturating_sub(cw))
    )
}

fn border_top() -> String {
    format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(BOX_W - 2))
}
fn border_mid() -> String {
    format!("\u{2560}{}\u{2563}", "\u{2550}".repeat(BOX_W - 2))
}
fn border_bot() -> String {
    format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(BOX_W - 2))
}

fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    matches!(
        c,
        '\u{2500}'..='\u{259F}'
            | '\u{2600}'..='\u{27BF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{3000}'..='\u{303F}'
            | '\u{FF00}'..='\u{FFEF}'
            | '\u{2E80}'..='\u{2EFF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{F900}'..='\u{FAFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padded_row_no_panic() {
        let long = "x".repeat(200);
        let row = padded_row(&long);
        assert!(row.starts_with('\u{2551}'));
        assert!(row.ends_with('\u{2551}'));
    }

    #[test]
    fn test_row_bar_zero_total() {
        let row = row_bar("test", 0, 0);
        assert!(row.contains("0.0%"));
    }

    #[test]
    fn test_display_width_cjk() {
        assert_eq!(display_width("认知"), 4);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("认知abc"), 7);
    }

    #[test]
    fn test_border_widths_consistent() {
        let top = border_top();
        let mid = border_mid();
        let bot = border_bot();
        assert_eq!(display_width(&top), display_width(&mid));
        assert_eq!(display_width(&mid), display_width(&bot));
    }
}
