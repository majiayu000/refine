mod cache;
mod policy;
mod profile_context;

use crate::lang::t;
use crate::score::{indicator_display, layer_display, ScoreResult};
use anyhow::Result;
use chrono::{DateTime, Utc};
use refine_core::infra::{llm_with_retry, LlmClient};
use std::sync::Arc;

pub(crate) use cache::{cache_current_deterministic, invalidate_cached, load_cached_for_score};
pub(crate) use policy::{deterministic_advice, portfolio_policy, PortfolioMode, PortfolioPolicy};
pub(crate) use profile_context::save_profile_context;

fn build_prompt(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    profile_context: Option<&str>,
) -> Result<(String, PortfolioPolicy)> {
    let policy = portfolio_policy(long_term, recent)?;
    let mut lines = Vec::new();
    lines.push(t!("Rolling-90-day metrics:", "滚动 90 天指标:").to_string());
    for layer in &long_term.layers {
        let indicators = layer
            .indicators
            .iter()
            .map(|indicator| {
                format!(
                    "{} {}",
                    indicator_display(&indicator.name),
                    indicator.display_value()
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!(
            "- {} [{}]: {}",
            layer_display(&layer.name),
            layer.signal.as_str(),
            indicators.join(", ")
        ));
    }

    if let Some(tension) = &long_term.tension {
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
            "Acknowledge the required policy in this exact JSON format (no markdown):\n\
             {\"policy\": \"<required policy key>\"}\n\
             The service renders the user-visible advice; do not return prose.",
            "用这个 JSON 格式确认强制政策（不要 markdown）：\n\
             {\"policy\": \"<强制政策键>\"}\n\
             用户可见建议由服务端渲染，不要返回自由文本。"
        )
    ));
    Ok((lines.join("\n"), policy))
}

fn system_prompt() -> &'static str {
    t!(
        "You validate a server-selected cognitive portfolio policy. Return only the requested policy JSON.",
        "你只负责确认服务端选定的认知项目组合政策。仅返回要求的政策 JSON。"
    )
}

fn render_policy_output(policy: &PortfolioPolicy, raw: &str) -> (String, String, bool) {
    let acknowledged = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get("policy")
                .and_then(|policy| policy.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|value| value == policy.mode.response_key());
    (
        policy::deterministic_short(policy.mode),
        deterministic_advice(policy),
        acknowledged,
    )
}

/// Ask the LLM to acknowledge the server-selected policy, then cache only the
/// deterministic server rendering. LLM free text never reaches user output.
pub async fn generate_and_cache(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    llm: &Arc<dyn LlmClient>,
    score_timestamp: DateTime<Utc>,
    long_cohort_identity: &str,
    recent_cohort_identity: &str,
) -> Result<String> {
    let profile_path = crate::config::mirror_dir().join("profile-summary.json");
    let profile_context = profile_context::load_profile_context_from_path(
        &profile_path,
        Utc::now(),
        long_cohort_identity,
    )?;
    let (prompt, policy) = build_prompt(long_term, recent, profile_context.as_deref())?;
    let model_identity = llm.cache_identity();
    let cache_key = cache::advice_cache_key(
        &prompt,
        system_prompt(),
        &model_identity,
        score_timestamp,
        long_cohort_identity,
        recent_cohort_identity,
    );
    let cache_path = crate::config::mirror_dir().join("advice.json");
    match cache::load_cached_for_key_from_path(&cache_path, &cache_key) {
        Ok(Some(cached)) => return Ok(cached.advice),
        Ok(None) => {}
        Err(error) => tracing::warn!("failed to load advice cache: {}", error),
    }

    let response = llm_with_retry(llm, &prompt, system_prompt())
        .await
        .map_err(|error| anyhow::anyhow!("LLM advice policy acknowledgement failed: {error}"))?;
    let (_, _, acknowledged) = render_policy_output(&policy, response.trim());
    if !acknowledged {
        tracing::warn!(
            "LLM did not acknowledge portfolio policy {}; using deterministic rendering",
            policy.mode.response_key()
        );
    }
    cache::save_policy_cache(
        &policy,
        &cache_key,
        &model_identity,
        score_timestamp,
        long_cohort_identity,
        recent_cohort_identity,
    )
}

#[cfg(test)]
mod tests {
    use super::policy::breadth_score;
    use super::*;
    use crate::score::Signal;

    #[test]
    fn prompt_requires_only_structured_policy_acknowledgement() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let (prompt, policy) = build_prompt(&long_term, &recent, None).unwrap();
        assert!(prompt.contains("promote_hold_stop"));
        assert!(prompt.contains("do not return prose"));
        assert_eq!(policy.mode, PortfolioMode::PromoteHoldStop);
    }

    #[test]
    fn free_text_bypass_negation_and_short_fields_never_reach_output() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let policy = portfolio_policy(&long_term, &recent).unwrap();
        let expected = (
            policy::deterministic_short(policy.mode),
            deterministic_advice(&policy),
        );
        let payloads = [
            r#"{"policy":"promote_hold_stop","short":"新 增 项 目","full":"扩大探索"}"#,
            r#"{"policy":"promote_hold_stop","short":"start new","full":"do not start a new project"}"#,
            r#"{"policy":"explore","short":"malicious","full":"start another project"}"#,
        ];

        for payload in payloads {
            let (short, full, _) = render_policy_output(&policy, payload);
            assert_eq!((short, full), expected);
        }
    }
}
