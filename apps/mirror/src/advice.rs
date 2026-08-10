use crate::lang::t;
use crate::score::{indicator_display, layer_display, ScoreResult, Signal};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::infra::{llm_with_retry, LlmClient};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

const ADVICE_CACHE_VERSION: &str = "advice-v2";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
/// 建议缓存超过这个时长即视为过期。
const ADVICE_STALE_AFTER_HOURS: i64 = 72;

#[derive(Debug, Serialize, Deserialize)]
pub struct CachedAdvice {
    pub advice: String,
    #[serde(default)]
    pub short: String,
    pub generated_at: DateTime<Utc>,
    #[serde(default)]
    pub cache_version: String,
    #[serde(default)]
    pub cache_key: String,
    #[serde(default)]
    pub model_identity: String,
}

impl CachedAdvice {
    /// 是否已过期。过期判定只有这一处，调用方各自决定怎么处理：
    /// 生成侧据此重新调用 LLM，展示侧据此打过期标记而不是静默显示空白。
    pub fn is_stale(&self) -> bool {
        (Utc::now() - self.generated_at).num_hours() >= ADVICE_STALE_AFTER_HOURS
    }
}

/// 读取缓存，**不过滤过期**。过期的建议依然返回，由调用方决定展示还是重生成；
/// 曾经这里对过期直接返回 None，导致 advice 停更两个月而界面上毫无痕迹。
pub fn load_cached() -> Result<Option<CachedAdvice>> {
    let path = crate::config::mirror_dir().join("advice.json");
    load_cached_from_path(&path)
}

fn load_cached_from_path(path: &Path) -> Result<Option<CachedAdvice>> {
    load_cached_matching_key(path, None)
}

fn load_cached_for_key_from_path(path: &Path, expected_key: &str) -> Result<Option<CachedAdvice>> {
    load_cached_matching_key(path, Some(expected_key))
}

fn load_cached_matching_key(
    path: &Path,
    expected_key: Option<&str>,
) -> Result<Option<CachedAdvice>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read advice cache {}: {}",
                path.display(),
                e
            ));
        }
    };
    let cached: CachedAdvice = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse advice cache JSON {}", path.display()))?;
    if cached.cache_version != ADVICE_CACHE_VERSION {
        return Ok(None);
    }
    if expected_key.is_some_and(|key| cached.cache_key != key) {
        return Ok(None);
    }
    // Generation only reuses a fresh cache entry. Display callers pass no key
    // and still receive stale advice so the UI can mark it explicitly.
    if expected_key.is_some() && cached.is_stale() {
        return Ok(None);
    }
    Ok(Some(cached))
}

fn save_cached(advice: &str, short: &str, cache_key: &str, model_identity: &str) -> Result<()> {
    let dir = crate::config::ensure_mirror_dir()?;
    let cached = CachedAdvice {
        advice: advice.to_string(),
        short: short.to_string(),
        generated_at: Utc::now(),
        cache_version: ADVICE_CACHE_VERSION.to_string(),
        cache_key: cache_key.to_string(),
        model_identity: model_identity.to_string(),
    };
    let json = serde_json::to_string_pretty(&cached)?;
    std::fs::write(dir.join("advice.json"), json)?;
    Ok(())
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
            layer.signal.as_str(),
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
pub async fn generate_and_cache(score: &ScoreResult, llm: &Arc<dyn LlmClient>) -> Result<String> {
    let prompt = build_prompt(score);
    let model_identity = llm.cache_identity();
    let cache_key = advice_cache_key(&prompt, system_prompt(), &model_identity);
    let cache_path = crate::config::mirror_dir().join("advice.json");
    match load_cached_for_key_from_path(&cache_path, &cache_key) {
        Ok(Some(cached)) => return Ok(cached.advice),
        Ok(None) => {}
        Err(err) => tracing::warn!("failed to load advice cache: {}", err),
    }

    let response = llm_with_retry(llm, &prompt, system_prompt())
        .await
        .map_err(|e| anyhow::anyhow!("LLM advice generation failed: {}", e))?;
    let raw = response.trim().to_string();

    // Parse JSON response: {"short": "...", "full": "..."}
    let (short, full) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
        let s = parsed
            .get("short")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let f = parsed
            .get("full")
            .and_then(|v| v.as_str())
            .unwrap_or(&raw)
            .to_string();
        (s, f)
    } else {
        // Fallback: LLM didn't return JSON, use first 15 chars as short
        let s: String = raw.chars().take(15).collect();
        (s, raw.clone())
    };

    save_cached(&full, &short, &cache_key, &model_identity)?;
    Ok(full)
}

fn advice_cache_key(prompt: &str, system: &str, model_identity: &str) -> String {
    format!(
        "{}:{:016x}",
        ADVICE_CACHE_VERSION,
        stable_hash(&[prompt, system, model_identity])
    )
}

fn stable_hash(parts: &[&str]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn cached_advice(advice: &str, generated_at: DateTime<Utc>) -> CachedAdvice {
        CachedAdvice {
            advice: advice.into(),
            short: advice.into(),
            generated_at,
            cache_version: ADVICE_CACHE_VERSION.into(),
            cache_key: "cache-key".into(),
            model_identity: "model".into(),
        }
    }

    #[test]
    fn test_load_cached_reports_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        std::fs::write(&path, "{\"advice\":").unwrap();

        let err = load_cached_from_path(&path).unwrap_err();
        assert!(err
            .to_string()
            .contains("failed to parse advice cache JSON"));
    }

    #[test]
    fn test_load_cached_returns_stale_cache_marked_stale() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let cached = cached_advice("stale", Utc::now() - Duration::hours(80));
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        // 过期缓存必须仍被返回并标记为 stale，展示侧才有机会打 ⚠️。
        // 旧行为是返回 None，等于把"建议停更"伪装成"本来就没有建议"。
        let loaded = load_cached_from_path(&path)
            .unwrap()
            .expect("过期缓存也必须返回，交由调用方判定");
        assert!(loaded.is_stale(), "80 小时前的缓存必须判为过期");
        assert_eq!(loaded.short, "stale");
    }

    #[test]
    fn test_load_cached_returns_fresh_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let cached = cached_advice("fresh", Utc::now() - Duration::hours(2));
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        let loaded = load_cached_from_path(&path).unwrap();
        assert_eq!(loaded.unwrap().advice, "fresh");
    }

    #[test]
    fn test_load_cached_returns_none_for_key_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let cached = cached_advice("fresh", Utc::now() - Duration::hours(2));
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        let loaded = load_cached_for_key_from_path(&path, "different-key").unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_load_cached_returns_none_for_legacy_cache_without_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "advice": "legacy",
                "short": "legacy",
                "generated_at": Utc::now(),
            })
            .to_string(),
        )
        .unwrap();

        let loaded = load_cached_from_path(&path).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_advice_cache_key_changes_with_prompt_and_model() {
        let base = advice_cache_key("prompt-a", system_prompt(), "openai:gpt-4o");
        let prompt_changed = advice_cache_key("prompt-b", system_prompt(), "openai:gpt-4o");
        let model_changed = advice_cache_key("prompt-a", system_prompt(), "openai:gpt-5");

        assert_ne!(base, prompt_changed);
        assert_ne!(base, model_changed);
    }
}
