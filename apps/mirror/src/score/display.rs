use crate::lang::t;

use super::baseline::PersonalTrends;
use super::indicators::indicator_display_name;
use super::types::{ScoreResult, Signal};

// ── Display helpers ──

pub fn layer_display(key: &str) -> &'static str {
    match key {
        "depth" => t!("Depth", "认知深度"),
        "breadth" => t!("Breadth", "战略广度"),
        "collaboration" => t!("Collaboration", "协作效能"),
        _ => "unknown",
    }
}

pub fn indicator_display(key: &str) -> &'static str {
    indicator_display_name(key)
}

// ── Output ──

pub(super) fn print_score(result: &ScoreResult, trends: Option<&PersonalTrends>) {
    println!("{}\n", t!("Mirror Cognitive Snapshot", "Mirror 认知镜像"));
    for layer in &result.layers {
        let details: Vec<String> = layer
            .indicators
            .iter()
            .map(|i| {
                let mark = if i.signal == Signal::Green {
                    "✓"
                } else {
                    "✗"
                };
                let arrow = trends
                    .and_then(|personal| personal.indicator(&i.name))
                    .map(|trend| trend.arrow())
                    .unwrap_or("");
                format!(
                    "{} {} {}{}",
                    indicator_display(&i.name),
                    i.display_value(),
                    mark,
                    arrow
                )
            })
            .collect();
        println!(
            "  {:<12} {}  {}",
            layer_display(&layer.name),
            layer.signal,
            details.join(" | ")
        );
    }
    if let Some(ref tension) = result.tension {
        println!("\n  {}{}", t!("Tension: ", "张力: "), tension);
    }
    if trends.is_some() {
        println!(
            "  {}",
            t!(
                "✓ = meets the absolute target · arrow = vs your 4-week average",
                "✓ = 达到绝对目标 · 箭头 = 相对你近 4 周均值"
            )
        );
    } else {
        println!(
            "  {}",
            t!(
                "✓ = meets the absolute target · trend unavailable (less than 4 weeks of data)",
                "✓ = 达到绝对目标 · 趋势不可用(数据不足4周)"
            )
        );
    }
}
