use crate::lang::t;
use crate::score::{indicator_display, layer_display, ScoreResult, Signal};
use anyhow::Result;
use chrono::{DateTime, Utc};
use refine_core::infra::LlmClient;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedAdvice {
    pub advice: String,
    pub generated_at: DateTime<Utc>,
}

pub fn load_cached() -> Option<CachedAdvice> {
    let path = crate::config::mirror_dir().join("advice.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let cached: CachedAdvice = serde_json::from_str(&content).ok()?;
    let age = Utc::now() - cached.generated_at;
    if age.num_hours() < 24 {
        Some(cached)
    } else {
        None
    }
}

fn save_cached(advice: &str) -> Result<()> {
    let dir = crate::config::ensure_mirror_dir()?;
    let cached = CachedAdvice {
        advice: advice.to_string(),
        generated_at: Utc::now(),
    };
    let json = serde_json::to_string_pretty(&cached)?;
    std::fs::write(dir.join("advice.json"), json)?;
    Ok(())
}

fn signal_label(s: Signal) -> &'static str {
    match s {
        Signal::Green => "green",
        Signal::Yellow => "yellow",
        Signal::Red => "red",
    }
}

fn build_prompt(score: &ScoreResult) -> String {
    let mut lines = Vec::new();
    lines.push(t!("Current metrics:", "当前指标:").to_string());
    for layer in &score.layers {
        let inds: Vec<String> = layer
            .indicators
            .iter()
            .map(|i| {
                let name = indicator_display(&i.name);
                match i.name.as_str() {
                    "mode_diversity" => format!("{} {}", name, i.actual as usize),
                    "bug_decision" => format!("{} {:.2}", name, i.actual),
                    "dreyfus" => format!("{} {:.1}", name, i.actual),
                    _ => format!("{} {:.0}%", name, i.actual),
                }
            })
            .collect();
        lines.push(format!(
            "- {} [{}]: {}",
            layer_display(&layer.name),
            signal_label(layer.signal),
            inds.join(", ")
        ));
    }

    if let Some(ref tension) = score.tension {
        lines.push(format!("\n{}{}", t!("Tension: ", "张力: "), tension));
    }

    // Find weakest
    let weakest = score
        .layers
        .iter()
        .flat_map(|l| &l.indicators)
        .filter(|i| i.signal == Signal::Red)
        .chain(
            score
                .layers
                .iter()
                .flat_map(|l| &l.indicators)
                .filter(|i| i.signal == Signal::Yellow),
        )
        .next();

    if let Some(w) = weakest {
        lines.push(format!(
            "\n{}{} = {:.1}",
            t!("Biggest concern: ", "最大问题: "),
            indicator_display(&w.name),
            w.actual
        ));
    }

    lines.push(format!(
        "\n{}",
        t!(
            "Give ONE specific, actionable suggestion for the next coding session. \
             Reference the actual numbers. Max 2 sentences. No fluff.",
            "给出一条具体可执行的建议，针对下一次编码会话。\
             引用实际数字。最多 2 句话。不要空话。"
        )
    ));
    lines.join("\n")
}

fn system_prompt() -> &'static str {
    t!(
        "You are a cognitive growth coach for a developer who uses AI coding tools. \
         Be direct and specific. Reference actual metrics. One suggestion only.",
        "你是开发者的认知成长教练。直接具体。引用实际指标。只给一条建议。"
    )
}

/// Generate advice via LLM (single attempt, best-effort) and cache result
pub async fn generate_and_cache(
    score: &ScoreResult,
    llm: &Arc<dyn LlmClient>,
) -> Result<String> {
    let prompt = build_prompt(score);
    let response = llm
        .complete(&prompt, Some(system_prompt()))
        .await
        .map_err(|e| anyhow::anyhow!("LLM advice generation failed: {}", e))?;
    let advice = response.trim().to_string();
    save_cached(&advice)?;
    Ok(advice)
}
