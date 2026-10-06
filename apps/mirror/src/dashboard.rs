use crate::cohort::{load_cohorts, EventWindow};
use crate::lang::t;
use crate::score::{self, ScoreResult};
use anyhow::{Context, Result};
use refine_core::knowledge::ItemRepository;
use refine_core::session::format_data_quality_stats;
use std::path::Path;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

const COGNITIVE: &[(&str, &str)] = &[
    ("expert", "expert"),
    ("proficient", "proficient"),
    ("competent", "competent"),
    ("adv_begin", "advanced_beginner"),
    ("novice", "novice"),
];
const COLLAB: &[(&str, &str)] = &[
    ("delegation", "delegation"),
    ("deep_inq", "deep_inquiry"),
    ("exploration", "exploration"),
    ("review", "review"),
    ("pair_prog", "pair_programming"),
    ("teaching", "teaching"),
    ("debugging", "debugging"),
];
const BAR_W: usize = 10;

fn box_w() -> usize {
    76
}

pub async fn handle_dashboard(
    repo: Arc<dyn ItemRepository>,
    since: Option<String>,
    all: bool,
    db_path: &Path,
) -> Result<()> {
    let canonical = !all && since.is_none();
    let now = chrono::Utc::now();
    let window = EventWindow::from_options(since.as_deref(), all, now)?;
    let windows = if canonical {
        vec![window, EventWindow::rolling(7, now)]
    } else {
        vec![window]
    };
    let mut cohorts = load_cohorts(repo.as_ref(), now, &windows)
        .await?
        .into_iter();
    let cohort = cohorts.next().expect("dashboard score window");
    let cluster = &cohort.cluster;
    let config = crate::config::load();
    let scope = canonical
        .then(|| score::ScoreScope::canonical(db_path, &config.targets, now, &cluster.data_quality))
        .transpose()?;
    if cluster.data_quality.input_observations == 0 {
        score::invalidate_empty_score_cache(scope.as_ref(), &crate::config::mirror_dir())?;
        println!(
            "{}",
            t!(
                "No observation data in the time window. Run `refine ingest-sessions` first.",
                "当前时间窗口内无观测数据。请先运行 `refine ingest-sessions` 导入会话。"
            )
        );
        return Ok(());
    }
    if cluster.data_quality.eligible_observations == 0 {
        score::invalidate_empty_score_cache(scope.as_ref(), &crate::config::mirror_dir())?;
        anyhow::bail!(
            "No eligible linked interactive observations in the dashboard window ({}); refusing to persist an empty score",
            format_data_quality_stats(&cluster.data_quality),
        );
    }
    let mut result = score::compute(cluster, &config.targets);
    result.timestamp = now;
    result.scope = scope;
    let published = if canonical {
        let recent = cohorts.next().expect("dashboard recent advice window");
        let recent_score = score::compute(&recent.cluster, &config.targets);
        Some(score::publish_canonical_score(
            &result,
            &recent_score,
            &recent.cluster.data_quality.cohort_identity,
            &crate::config::mirror_dir(),
        )?)
    } else {
        None
    };

    let stats = &cluster.global_stats;
    let cog_total: usize = stats.cognitive_levels.values().sum();
    let collab_total: usize = stats.collaboration_modes.values().sum();
    let w = box_w();

    p(&border_top(w));
    p(&row_center(
        t!("Mirror Cognitive Dashboard", "Mirror 认知仪表盘"),
        w,
    ));
    p(&border_mid(w));

    let pad: usize = t!(14, 10);
    for layer in &result.layers {
        let details: Vec<String> = layer.indicators.iter().map(format_indicator).collect();
        let line = format!(
            " {:<pad$} {}  {}",
            score::layer_display(&layer.name),
            layer.signal,
            details.join(" | "),
            pad = pad
        );
        p(&padded_row(&line, w));
    }
    p(&border_mid(w));

    let tension_text = match result.tension {
        Some(ref t) => format!("{}{}", t!("Tension: ", "张力: "), t),
        None => t!("Tension: no obvious conflict", "张力: 无明显维度冲突").to_string(),
    };
    p(&padded_row(
        &format!(" {}", truncate(&tension_text, w - 4)),
        w,
    ));
    p(&border_mid(w));

    p(&padded_row(
        t!(" Cognitive Level Distribution", " 认知水平分布"),
        w,
    ));
    for (label, key) in COGNITIVE {
        let n = stats.cognitive_levels.get(*key).copied().unwrap_or(0);
        p(&row_bar(label, n, cog_total, w));
    }
    p(&border_mid(w));

    p(&padded_row(t!(" Collaboration Modes", " 协作模式"), w));
    for (label, key) in COLLAB {
        let n = stats.collaboration_modes.get(*key).copied().unwrap_or(0);
        p(&row_bar(label, n, collab_total, w));
    }
    p(&border_mid(w));

    if canonical {
        print_trend(&result, w)?;
    } else {
        p(&padded_row(
            t!(
                " View only: no canonical history update",
                " 仅查看：不更新标准评分历史"
            ),
            w,
        ));
    }
    p(&border_bot(w));
    if let Some(published) = published {
        if let Ok(advice) = published.advice {
            println!("{} {}", t!("Advice:", "建议:"), advice);
        }
    }
    println!(
        "{} {}",
        t!("Data quality:", "数据质量:"),
        format_data_quality_stats(&cluster.data_quality)
    );
    Ok(())
}

fn p(s: &str) {
    println!("{s}");
}

fn format_indicator(ind: &score::Indicator) -> String {
    format!(
        "{} {}{}",
        score::indicator_display(&ind.name),
        ind.display_value(),
        ind.coverage_label()
    )
}

fn print_trend(current: &ScoreResult, w: usize) -> Result<()> {
    let scope = current
        .scope
        .as_ref()
        .context("dashboard trend requires canonical scope")?;
    let mut history = score::load_recent_scores(5, scope)?;
    if history.is_empty() {
        history.push(current.clone());
    }
    p(&padded_row(t!(" Recent Score Trend", " 最近评分趋势"), w));
    let entries: Vec<String> = history
        .iter()
        .map(|s| {
            let date = s.timestamp.format("%m-%d");
            let lights: String = s
                .layers
                .iter()
                .map(|l| l.signal.to_string())
                .collect::<Vec<_>>()
                .join("");
            format!("{} {}", date, lights)
        })
        .collect();
    p(&padded_row(&format!("  {}", entries.join("  ")), w));
    Ok(())
}

fn truncate(s: &str, max_w: usize) -> String {
    let mut w = 0;
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        let cw = c.width().unwrap_or(0);
        if w + cw > max_w {
            end = i;
            break;
        }
        w += cw;
    }
    if end < s.len() {
        format!("{}…", &s[..end])
    } else {
        s.to_string()
    }
}

// ── Box drawing ──

fn row_bar(label: &str, count: usize, total: usize, w: usize) -> String {
    if total == 0 {
        return padded_row(&format!("  {:<11} {} (0)", label, t!("n/a", "证据不足")), w);
    }
    let ratio = count as f64 / total as f64;
    let filled = (ratio * BAR_W as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(BAR_W.saturating_sub(filled)),
    );
    padded_row(
        &format!(
            "  {:<11} {} {:>5.1}% ({:>3})",
            label,
            bar,
            ratio * 100.0,
            count
        ),
        w,
    )
}

fn row_center(text: &str, w: usize) -> String {
    let tw = display_width(text);
    let inner = w.saturating_sub(2);
    let left = inner.saturating_sub(tw) / 2;
    let right = inner.saturating_sub(tw).saturating_sub(left);
    format!(
        "\u{2551}{}{}{}\u{2551}",
        " ".repeat(left),
        text,
        " ".repeat(right)
    )
}

fn padded_row(content: &str, w: usize) -> String {
    let cw = display_width(content);
    let inner = w.saturating_sub(2);
    format!(
        "\u{2551}{}{}\u{2551}",
        content,
        " ".repeat(inner.saturating_sub(cw))
    )
}

fn border_top(w: usize) -> String {
    format!("\u{2554}{}\u{2557}", "\u{2550}".repeat(w - 2))
}
fn border_mid(w: usize) -> String {
    format!("\u{2560}{}\u{2563}", "\u{2550}".repeat(w - 2))
}
fn border_bot(w: usize) -> String {
    format!("\u{255A}{}\u{255D}", "\u{2550}".repeat(w - 2))
}

/// Display width using unicode-width, skipping ANSI escape sequences
fn display_width(s: &str) -> usize {
    let mut w = 0;
    let mut in_esc = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_esc = true;
            continue;
        }
        if in_esc {
            if c.is_ascii_alphabetic() {
                in_esc = false;
            }
            continue;
        }
        w += c.width().unwrap_or(0);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padded_row_no_panic() {
        let long = "x".repeat(200);
        let row = padded_row(&long, 76);
        assert!(row.starts_with('\u{2551}'));
        assert!(row.ends_with('\u{2551}'));
    }

    #[test]
    fn test_row_bar_zero_total() {
        let row = row_bar("test", 0, 0, 76);
        assert!(!row.contains("0.0%"));
        assert!(row.contains(t!("n/a", "证据不足")));
    }

    #[test]
    fn test_display_width_cjk() {
        assert_eq!(display_width("认知"), 4);
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("认知abc"), 7);
    }

    #[test]
    fn test_display_width_skips_ansi() {
        assert_eq!(display_width("\x1b[32m●\x1b[0m"), 1);
        assert_eq!(display_width("ab\x1b[31mcd\x1b[0mef"), 6);
    }

    #[test]
    fn test_border_widths_consistent() {
        let w = 76;
        let top = border_top(w);
        let mid = border_mid(w);
        let bot = border_bot(w);
        assert_eq!(display_width(&top), display_width(&mid));
        assert_eq!(display_width(&mid), display_width(&bot));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello…");
    }

    #[tokio::test]
    async fn dashboard_handler_rejects_detached_only_cohort_before_persist() {
        let (_fixture, store) = crate::test_support::legacy_detached_store();

        let error = handle_dashboard(store, None, true, Path::new("synthetic.db"))
            .await
            .expect_err("detached-only cohort must fail closed");

        assert!(error.to_string().contains("No eligible linked"));
        assert!(error.to_string().contains("refusing to persist"));
    }
}
