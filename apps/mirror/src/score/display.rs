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

    println!("  {}", trend_legend(trends.is_some()));
}

fn trend_legend(available: bool) -> &'static str {
    if available {
        t!(
            "✓ = meets the absolute target · arrow = vs your 4-week average",
            "✓ = 达到绝对目标 · 箭头 = 相对你近 4 周均值"
        )
    } else {
        t!(
            "✓ = meets the absolute target · trend unavailable (requires 7 distinct eligible scoring dates in the past 28 days)",
            "✓ = 达到绝对目标 · 趋势不可用(近28天内需有7个不同日期的有效评分)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::trend_legend;

    #[test]
    fn unavailable_legend_describes_distinct_dates_in_lookback_window() {
        let legend = trend_legend(false);
        assert!(legend.contains("7 distinct"), "{legend}");
        assert!(legend.contains("28 days"), "{legend}");
        assert!(!legend.contains("less than 4 weeks"), "{legend}");
        assert!(trend_legend(true).contains("average"));
    }
}
