use crate::lang::t;
use crate::score::{indicator_display, layer_display, Indicator, ScoreResult, Signal};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use refine_core::infra::{llm_with_retry, LlmClient};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

const ADVICE_CACHE_VERSION: &str = "advice-score-v4";
const PROFILE_CONTEXT_SCHEMA_VERSION: u32 = 1;
const PROFILE_CONTEXT_SOURCE_REVISION: &str = "mirror-profile-context-v1";
const PROFILE_CONTEXT_STALE_AFTER_DAYS: i64 = 14;
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const ADVICE_STALE_AFTER_HOURS: i64 = 72;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PortfolioMode {
    PromoteHoldStop,
    Explore,
    Deepen,
}

impl PortfolioMode {
    fn response_key(self) -> &'static str {
        match self {
            Self::PromoteHoldStop => "promote_hold_stop",
            Self::Explore => "explore",
            Self::Deepen => "deepen",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PortfolioPolicy {
    pub mode: PortfolioMode,
    pub long_exploration: Indicator,
    pub long_fragmentation: Indicator,
    pub recent_exploration: Indicator,
    pub recent_fragmentation: Indicator,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileContext {
    schema_version: u32,
    source_revision: String,
    generated_at: DateTime<Utc>,
    window: String,
    cohort_identity: String,
    summary: String,
}

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
    pub fn is_stale(&self) -> bool {
        (Utc::now() - self.generated_at).num_hours() >= ADVICE_STALE_AFTER_HOURS
    }
}

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

pub(crate) fn save_profile_context(
    summary: &str,
    window: &str,
    cohort_identity: &str,
) -> Result<()> {
    let dir = crate::config::ensure_mirror_dir()?;
    let context = ProfileContext {
        schema_version: PROFILE_CONTEXT_SCHEMA_VERSION,
        source_revision: PROFILE_CONTEXT_SOURCE_REVISION.to_string(),
        generated_at: Utc::now(),
        window: window.to_string(),
        cohort_identity: cohort_identity.to_string(),
        summary: summary.to_string(),
    };
    let json = serde_json::to_string_pretty(&context)?;
    std::fs::write(dir.join("profile-summary.json"), json)
        .context("failed to write versioned profile summary")?;
    Ok(())
}

fn load_profile_context_from_path(path: &Path, now: DateTime<Utc>) -> Result<Option<String>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::anyhow!(
                "failed to read profile context {}: {}",
                path.display(),
                error
            ));
        }
    };
    let context: ProfileContext = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse profile context JSON {}", path.display()))?;
    if context.schema_version != PROFILE_CONTEXT_SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported profile context schema {} in {}; expected {}",
            context.schema_version,
            path.display(),
            PROFILE_CONTEXT_SCHEMA_VERSION
        );
    }
    if context.source_revision != PROFILE_CONTEXT_SOURCE_REVISION {
        anyhow::bail!(
            "unsupported profile context source revision '{}' in {}; expected '{}'",
            context.source_revision,
            path.display(),
            PROFILE_CONTEXT_SOURCE_REVISION
        );
    }
    if context.generated_at > now {
        anyhow::bail!(
            "profile context {} has a future generated_at timestamp",
            path.display()
        );
    }
    if (now - context.generated_at).num_days() >= PROFILE_CONTEXT_STALE_AFTER_DAYS {
        return Ok(None);
    }
    if context.window.trim().is_empty()
        || context.cohort_identity.trim().is_empty()
        || context.summary.trim().is_empty()
    {
        anyhow::bail!(
            "profile context {} is missing window, cohort identity, or summary",
            path.display()
        );
    }
    Ok(Some(format!(
        "generated_at={}\nwindow={}\nschema_version={}\nsource_revision={}\ncohort_identity={}\n{}",
        context.generated_at.to_rfc3339(),
        context.window,
        context.schema_version,
        context.source_revision,
        context.cohort_identity,
        context.summary
    )))
}

fn breadth_indicator(score: &ScoreResult, name: &str, window: &str) -> Result<Indicator> {
    score
        .layers
        .iter()
        .find(|layer| layer.name == "breadth")
        .and_then(|layer| {
            layer
                .indicators
                .iter()
                .find(|indicator| indicator.name == name)
        })
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "portfolio advice requires '{}' in the {} breadth metrics",
                name,
                window
            )
        })
}

pub(crate) fn portfolio_policy(
    long_term: &ScoreResult,
    recent: &ScoreResult,
) -> Result<PortfolioPolicy> {
    let long_exploration = breadth_indicator(long_term, "exploration", "rolling-90-day")?;
    let long_fragmentation = breadth_indicator(long_term, "fragmentation", "rolling-90-day")?;
    let recent_exploration = breadth_indicator(recent, "exploration", "rolling-7-day")?;
    let recent_fragmentation = breadth_indicator(recent, "fragmentation", "rolling-7-day")?;

    let fragmentation_non_green =
        long_fragmentation.signal != Signal::Green || recent_fragmentation.signal != Signal::Green;
    let both_exploration_low =
        long_exploration.signal != Signal::Green && recent_exploration.signal != Signal::Green;
    let mode = if fragmentation_non_green {
        PortfolioMode::PromoteHoldStop
    } else if both_exploration_low {
        PortfolioMode::Explore
    } else {
        PortfolioMode::Deepen
    };

    Ok(PortfolioPolicy {
        mode,
        long_exploration,
        long_fragmentation,
        recent_exploration,
        recent_fragmentation,
    })
}

pub(crate) fn deterministic_advice(policy: &PortfolioPolicy) -> String {
    match policy.mode {
        PortfolioMode::PromoteHoldStop => t!(
            format!(
                "Promote / Hold / Stop: promote the strongest evidenced project, hold at most one bounded validation, and stop the weakest one-off thread unless it produces a named result this week. One-off share is {:.1}% over 90 days and {:.1}% over 7 days; keep the active portfolio closed to additions.",
                policy.long_fragmentation.actual, policy.recent_fragmentation.actual
            ),
            format!(
                "晋升 / 保留 / 退出：晋升证据最强的项目，最多保留一项有边界的验证；最弱的一次性线程若本周没有产出具名结果就退出。90 天与 7 天的一次性项目占比分别为 {:.1}% 和 {:.1}%，本周项目组合不增加任何条目。",
                policy.long_fragmentation.actual, policy.recent_fragmentation.actual
            )
        ),
        PortfolioMode::Explore => t!(
            format!(
                "Run one bounded exploration inside an existing project and record a keep/stop decision. Exploration is {:.1}% over 90 days and {:.1}% over 7 days while fragmentation remains green in both windows.",
                policy.long_exploration.actual, policy.recent_exploration.actual
            ),
            format!(
                "在现有项目内做一次有边界的探索，并记录保留或退出决定。90 天与 7 天探索率分别为 {:.1}% 和 {:.1}%，且两个窗口的碎片化均为绿灯。",
                policy.long_exploration.actual, policy.recent_exploration.actual
            )
        ),
        PortfolioMode::Deepen => t!(
            format!(
                "Hold the current portfolio and deepen the strongest active project with one named validation. Exploration is {:.1}% over 90 days and {:.1}% over 7 days, so expansion is not the priority.",
                policy.long_exploration.actual, policy.recent_exploration.actual
            ),
            format!(
                "保持当前项目组合，在证据最强的活跃项目中完成一项具名验证。90 天与 7 天探索率分别为 {:.1}% 和 {:.1}%，扩张不是当前优先级。",
                policy.long_exploration.actual, policy.recent_exploration.actual
            )
        ),
    }
}

fn build_prompt(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    profile_context: Option<&str>,
) -> Result<(String, PortfolioPolicy)> {
    let policy = portfolio_policy(long_term, recent)?;
    let mut lines = Vec::new();
    lines.push(t!("Rolling-90-day metrics:", "滚动 90 天指标:").to_string());
    for layer in &long_term.layers {
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

    if let Some(ref tension) = long_term.tension {
        lines.push(format!("\n{}{}", t!("Tension: ", "张力: "), tension));
    }
    lines.push(format!(
        "\n{} exploration={:.1}% ({}), fragmentation={:.1}% ({})",
        t!("Rolling-7-day breadth:", "滚动 7 天广度:"),
        policy.recent_exploration.actual,
        policy.recent_exploration.signal.as_str(),
        policy.recent_fragmentation.actual,
        policy.recent_fragmentation.signal.as_str()
    ));
    lines.push(format!(
        "\n{} {}\n{}",
        t!("Required portfolio policy:", "强制项目组合政策:"),
        policy.mode.response_key(),
        deterministic_advice(&policy)
    ));

    if let Some(summary) = profile_context {
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
             {\"policy\": \"<required policy key>\", \"short\": \"<8 words max, one actionable verb phrase>\", \"full\": \"<1-2 sentences with actual numbers>\"}\n\
             Do not contradict the required portfolio policy.",
            "用这个 JSON 格式回复（不要 markdown）：\n\
             {\"policy\": \"<强制政策键>\", \"short\": \"<最多10个字，一个动作短语>\", \"full\": \"<1-2句话，引用实际数字>\"}\n\
             不得违背强制项目组合政策。"
        )
    ));
    Ok((lines.join("\n"), policy))
}

fn system_prompt() -> &'static str {
    t!(
        "You are a cognitive growth coach for a developer who uses AI coding tools. \
         Be direct and specific. Reference actual metrics. One suggestion only.",
        "你是开发者的认知成长教练。直接具体。引用实际指标。只给一条建议。"
    )
}

/// Generate guarded advice via LLM and cache the policy-compliant result.
pub async fn generate_and_cache(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    llm: &Arc<dyn LlmClient>,
) -> Result<String> {
    let profile_path = crate::config::mirror_dir().join("profile-summary.json");
    let profile_context = load_profile_context_from_path(&profile_path, Utc::now())?;
    let (prompt, policy) = build_prompt(long_term, recent, profile_context.as_deref())?;
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

    let fallback = deterministic_advice(&policy);
    let (short, full) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
        let response_policy = parsed
            .get("policy")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let s = parsed
            .get("short")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let f = parsed
            .get("full")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if response_policy == policy.mode.response_key()
            && !s.trim().is_empty()
            && response_matches_policy(&f, policy.mode)
        {
            (s, f)
        } else {
            (fallback_short(policy.mode), fallback.clone())
        }
    } else {
        (fallback_short(policy.mode), fallback)
    };

    save_cached(&full, &short, &cache_key, &model_identity)?;
    Ok(full)
}

fn fallback_short(mode: PortfolioMode) -> String {
    match mode {
        PortfolioMode::PromoteHoldStop => t!("Promote hold stop", "晋升保留退出").to_string(),
        PortfolioMode::Explore => t!("Bound one exploration", "限定一次探索").to_string(),
        PortfolioMode::Deepen => t!("Deepen current portfolio", "深挖当前组合").to_string(),
    }
}

fn response_matches_policy(response: &str, mode: PortfolioMode) -> bool {
    if response.trim().is_empty() {
        return false;
    }
    let normalized = response.to_lowercase();
    let forbidden_addition = [
        "new project",
        "one-off experiment",
        "start another",
        "start a project",
        "新增项目",
        "新项目",
        "一次性实验",
    ];
    let forbidden_direction = ["new direction", "expand exploration", "新方向", "扩大探索"];
    let has_forbidden_addition = forbidden_addition
        .iter()
        .any(|phrase| normalized.contains(phrase));
    let has_forbidden_direction = forbidden_direction
        .iter()
        .any(|phrase| normalized.contains(phrase));
    let has_decisions = (["promote", "晋升"]
        .iter()
        .any(|term| normalized.contains(term)))
        && (["hold", "保留"]
            .iter()
            .any(|term| normalized.contains(term)))
        && (["stop", "退出"]
            .iter()
            .any(|term| normalized.contains(term)));
    match mode {
        PortfolioMode::PromoteHoldStop => {
            !has_forbidden_addition && !has_forbidden_direction && has_decisions
        }
        PortfolioMode::Explore => !has_forbidden_addition,
        PortfolioMode::Deepen => !has_forbidden_addition && !has_forbidden_direction,
    }
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

    fn indicator(name: &str, actual: f64, signal: Signal) -> Indicator {
        Indicator {
            name: name.into(),
            actual,
            target: String::new(),
            signal,
        }
    }

    fn breadth_score(
        exploration: f64,
        exploration_signal: Signal,
        fragmentation: f64,
        fragmentation_signal: Signal,
    ) -> ScoreResult {
        let mut score = ScoreResult::default();
        score.layers[1].indicators = vec![
            indicator("exploration", exploration, exploration_signal),
            indicator("fragmentation", fragmentation, fragmentation_signal),
        ];
        score
    }

    fn profile_context(generated_at: DateTime<Utc>) -> ProfileContext {
        ProfileContext {
            schema_version: PROFILE_CONTEXT_SCHEMA_VERSION,
            source_revision: PROFILE_CONTEXT_SOURCE_REVISION.into(),
            generated_at,
            window: "all eligible linked interactive observations".into(),
            cohort_identity: "sha256:test".into(),
            summary: "12 sessions, 2 projects".into(),
        }
    }

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
    fn test_load_cached_returns_stale_cache_for_display_callers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let cached = cached_advice("stale", Utc::now() - Duration::hours(80));
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        let loaded = load_cached_from_path(&path)
            .unwrap()
            .expect("display callers need stale cache visibility");
        assert!(loaded.is_stale());
        assert_eq!(loaded.advice, "stale");
    }

    #[test]
    fn test_load_cached_for_generation_ignores_stale_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let cached = cached_advice("stale", Utc::now() - Duration::hours(80));
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        let loaded = load_cached_for_key_from_path(&path, "cache-key").unwrap();
        assert!(loaded.is_none());
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
    fn test_load_cached_rejects_previous_score_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let mut cached = cached_advice("old score advice", Utc::now() - Duration::hours(2));
        cached.cache_version = "advice-v2".into();
        std::fs::write(&path, serde_json::to_string(&cached).unwrap()).unwrap();

        assert!(load_cached_from_path(&path).unwrap().is_none());
    }

    #[test]
    fn test_advice_cache_key_changes_with_prompt_and_model() {
        let base = advice_cache_key("prompt-a", system_prompt(), "openai:gpt-4o");
        let prompt_changed = advice_cache_key("prompt-b", system_prompt(), "openai:gpt-4o");
        let model_changed = advice_cache_key("prompt-a", system_prompt(), "openai:gpt-5");

        assert_ne!(base, prompt_changed);
        assert_ne!(base, model_changed);
    }

    #[test]
    fn portfolio_policy_matrix_prioritizes_fragmentation_over_exploration() {
        let green = breadth_score(20.0, Signal::Green, 5.0, Signal::Green);
        let low = breadth_score(10.0, Signal::Red, 5.0, Signal::Green);
        let fragmented = breadth_score(30.0, Signal::Green, 35.0, Signal::Red);

        assert_eq!(
            portfolio_policy(&green, &fragmented).unwrap().mode,
            PortfolioMode::PromoteHoldStop
        );
        assert_eq!(
            portfolio_policy(&fragmented, &green).unwrap().mode,
            PortfolioMode::PromoteHoldStop
        );
        assert_eq!(
            portfolio_policy(&low, &low).unwrap().mode,
            PortfolioMode::Explore
        );
        assert_eq!(
            portfolio_policy(&low, &green).unwrap().mode,
            PortfolioMode::Deepen
        );
    }

    #[test]
    fn regression_14_4_29_2_and_high_one_off_never_expands() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let policy = portfolio_policy(&long_term, &recent).unwrap();
        let fallback = deterministic_advice(&policy);

        assert_eq!(policy.mode, PortfolioMode::PromoteHoldStop);
        assert!(fallback.contains("Promote / Hold / Stop"));
        assert!(!fallback.to_lowercase().contains("new project"));
        assert!(!fallback.to_lowercase().contains("new direction"));
    }

    #[test]
    fn policy_fails_clearly_when_a_required_window_metric_is_missing() {
        let missing = ScoreResult::default();
        let valid = breadth_score(20.0, Signal::Green, 5.0, Signal::Green);
        let error = portfolio_policy(&missing, &valid).unwrap_err();
        assert!(error.to_string().contains("rolling-90-day"));
        assert!(error.to_string().contains("exploration"));
    }

    #[test]
    fn prompt_and_guard_share_the_computed_policy() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let (prompt, policy) = build_prompt(&long_term, &recent, None).unwrap();
        assert!(prompt.contains("promote_hold_stop"));
        assert_eq!(policy.mode, PortfolioMode::PromoteHoldStop);
        assert!(response_matches_policy(
            "Promote core, hold one validation, stop side work.",
            policy.mode
        ));
        assert!(!response_matches_policy(
            "Promote core, hold one validation, stop side work, then start a new project.",
            policy.mode
        ));
        assert!(!response_matches_policy(
            "Deepen the core and start a new direction.",
            PortfolioMode::Deepen
        ));
    }

    #[test]
    fn fresh_profile_context_includes_reproducibility_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        let now = Utc::now();
        std::fs::write(
            &path,
            serde_json::to_string(&profile_context(now - Duration::days(2))).unwrap(),
        )
        .unwrap();

        let loaded = load_profile_context_from_path(&path, now)
            .unwrap()
            .expect("fresh profile context");
        assert!(loaded.contains("generated_at="));
        assert!(loaded.contains("window="));
        assert!(loaded.contains("schema_version=1"));
        assert!(loaded.contains("source_revision=mirror-profile-context-v1"));
        assert!(loaded.contains("cohort_identity=sha256:test"));
    }

    #[test]
    fn stale_profile_context_is_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        let now = Utc::now();
        std::fs::write(
            &path,
            serde_json::to_string(&profile_context(now - Duration::days(14))).unwrap(),
        )
        .unwrap();
        assert!(load_profile_context_from_path(&path, now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_or_unverifiable_profile_context_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        std::fs::write(&path, "legacy text without metadata").unwrap();
        assert!(load_profile_context_from_path(&path, Utc::now())
            .unwrap_err()
            .to_string()
            .contains("failed to parse profile context JSON"));

        let mut unsupported = profile_context(Utc::now() - Duration::days(1));
        unsupported.source_revision = "unknown-revision".into();
        std::fs::write(&path, serde_json::to_string(&unsupported).unwrap()).unwrap();
        assert!(load_profile_context_from_path(&path, Utc::now())
            .unwrap_err()
            .to_string()
            .contains("unsupported profile context source revision"));
    }
}
