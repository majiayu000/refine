use super::types::{ScoreResult, Signal};
use crate::lang::t;

pub fn layer_display(key: &str) -> &'static str {
    match key {
        "depth" => t!("Depth", "认知深度"),
        "breadth" => t!("Breadth", "战略广度"),
        "collaboration" => t!("Collaboration", "协作效能"),
        _ => "unknown",
    }
}

pub fn indicator_display(key: &str) -> &'static str {
    match key {
        "dreyfus" => "Dreyfus",
        "decision_quality" => t!("Decision Quality", "决策质量"),
        "depth_output" => t!("Depth Output", "深度产出比"),
        "exploration" => t!("Exploration", "探索率"),
        "deep_invest" => t!("Deep Invest", "深耕率"),
        "fragmentation" => t!("Fragmentation", "碎片化"),
        "delegation" => "delegation",
        "mode_diversity" => t!("Mode Diversity", "模式多样性"),
        "bug_decision" => "bug/decision",
        "knowledge_rate" => t!("Knowledge", "知识获取"),
        "friction_density" => t!("Friction", "摩擦密度"),
        _ => "unknown",
    }
}

pub(super) fn print_score(result: &ScoreResult, using_personal: bool) {
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
                format!(
                    "{} {} {}",
                    indicator_display(&i.name),
                    i.display_value(),
                    mark
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
    if using_personal {
        println!(
            "  {}",
            t!(
                "Baseline: personal (4-week rolling avg)",
                "基线: 个人(4周均值)"
            )
        );
    } else {
        println!(
            "  {}",
            t!(
                "Baseline: default thresholds (insufficient data for personal baseline)",
                "基线: 默认阈值(数据不足4周)"
            )
        );
    }
}
