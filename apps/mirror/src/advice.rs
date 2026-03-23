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
    #[serde(default)]
    pub short: String,
    pub generated_at: DateTime<Utc>,
}

pub fn load_cached() -> Option<CachedAdvice> {
    let path = crate::config::mirror_dir().join("advice.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let cached: CachedAdvice = serde_json::from_str(&content).ok()?;
    let age = Utc::now() - cached.generated_at;
    if age.num_hours() < 72 {
        Some(cached)
    } else {
        None
    }
}

fn save_cached(advice: &str, short: &str) -> Result<()> {
    let dir = crate::config::ensure_mirror_dir()?;
    let cached = CachedAdvice {
        advice: advice.to_string(),
        short: short.to_string(),
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
            .map(|i| format!("{} {}", indicator_display(&i.name), i.display_value()))
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

    // Append profile context if available
    let profile_path = crate::config::mirror_dir().join("profile-summary.txt");
    if let Ok(summary) = std::fs::read_to_string(&profile_path) {
        lines.push(format!(
            "\n{}\n{}",
            t!("Profile context:", "画像上下文:"),
            summary
        ));
    }

    lines.push(format!(
        "\n{}",
        t!(
            "Respond in this exact JSON format (no markdown):\n\
             {\"short\": \"<8 words max, one actionable verb phrase>\", \"full\": \"<1-2 sentences with actual numbers>\"}\n\
             Example: {\"short\": \"Write 3 design decisions before coding\", \"full\": \"Your Decision Quality is 41%...\"}",
            "用这个 JSON 格式回复（不要 markdown）：\n\
             {\"short\": \"<最多10个字，一个动作短语>\", \"full\": \"<1-2句话，引用实际数字>\"}\n\
             示例：{\"short\": \"开工前写3个设计决策\", \"full\": \"你的决策质量只有41%...\"}"
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
    let raw = response.trim().to_string();

    // Parse JSON response: {"short": "...", "full": "..."}
    let (short, full) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
        let s = parsed.get("short").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let f = parsed.get("full").and_then(|v| v.as_str()).unwrap_or(&raw).to_string();
        (s, f)
    } else {
        // Fallback: LLM didn't return JSON, use first 15 chars as short
        let s: String = raw.chars().take(15).collect();
        (s, raw.clone())
    };

    save_cached(&full, &short)?;
    Ok(full)
}
