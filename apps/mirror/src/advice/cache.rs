use super::policy::{deterministic_advice, deterministic_short, portfolio_policy, PortfolioPolicy};
use super::profile_context::validate_cohort_identity;
use crate::score::ScoreResult;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

const CACHE_VERSION: &str = "advice-score-v5";
const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;
const STALE_AFTER_HOURS: i64 = 72;

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
    #[serde(default = "legacy_timestamp")]
    pub score_timestamp: DateTime<Utc>,
    #[serde(default)]
    pub policy_key: String,
    #[serde(default)]
    pub long_cohort_identity: String,
    #[serde(default)]
    pub recent_cohort_identity: String,
}

fn legacy_timestamp() -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH
}

impl CachedAdvice {
    pub fn is_stale(&self) -> bool {
        (Utc::now() - self.generated_at).num_hours() >= STALE_AFTER_HOURS
    }
}

pub fn load_cached_for_score(score: &ScoreResult) -> Result<Option<CachedAdvice>> {
    let path = crate::config::mirror_dir().join("advice.json");
    load_cached_matching(&path, None, Some(score.timestamp))
}

pub(super) fn load_cached_for_key_from_path(
    path: &Path,
    expected_key: &str,
) -> Result<Option<CachedAdvice>> {
    load_cached_matching(path, Some(expected_key), None)
}

fn load_cached_matching(
    path: &Path,
    expected_key: Option<&str>,
    expected_score_timestamp: Option<DateTime<Utc>>,
) -> Result<Option<CachedAdvice>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::anyhow!(
                "failed to read advice cache {}: {}",
                path.display(),
                error
            ));
        }
    };
    let cached: CachedAdvice = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse advice cache JSON {}", path.display()))?;
    if cached.cache_version != CACHE_VERSION
        || expected_key.is_some_and(|key| cached.cache_key != key)
        || expected_score_timestamp.is_some_and(|timestamp| cached.score_timestamp != timestamp)
    {
        return Ok(None);
    }
    if expected_key.is_some() && cached.is_stale() {
        return Ok(None);
    }
    Ok(Some(cached))
}

pub(super) fn save_policy_cache(
    policy: &PortfolioPolicy,
    cache_key: &str,
    model_identity: &str,
    score_timestamp: DateTime<Utc>,
    long_cohort_identity: &str,
    recent_cohort_identity: &str,
) -> Result<String> {
    validate_cohort_identity(long_cohort_identity, "rolling-90-day")?;
    validate_cohort_identity(recent_cohort_identity, "rolling-7-day")?;
    let dir = crate::config::ensure_mirror_dir()?;
    let advice = deterministic_advice(policy);
    let cached = CachedAdvice {
        advice: advice.clone(),
        short: deterministic_short(policy.mode),
        generated_at: Utc::now(),
        cache_version: CACHE_VERSION.to_string(),
        cache_key: cache_key.to_string(),
        model_identity: model_identity.to_string(),
        score_timestamp,
        policy_key: policy.mode.response_key().to_string(),
        long_cohort_identity: long_cohort_identity.to_string(),
        recent_cohort_identity: recent_cohort_identity.to_string(),
    };
    let json = serde_json::to_string_pretty(&cached)?;
    std::fs::write(dir.join("advice.json"), json)?;
    Ok(advice)
}

pub(crate) fn cache_current_deterministic(
    long_term: &ScoreResult,
    recent: &ScoreResult,
    score_timestamp: DateTime<Utc>,
    long_cohort_identity: &str,
    recent_cohort_identity: &str,
) -> Result<String> {
    let policy = portfolio_policy(long_term, recent)?;
    let cache_key = advice_cache_key(
        policy.mode.response_key(),
        "deterministic",
        "deterministic",
        score_timestamp,
        long_cohort_identity,
        recent_cohort_identity,
    );
    save_policy_cache(
        &policy,
        &cache_key,
        "deterministic",
        score_timestamp,
        long_cohort_identity,
        recent_cohort_identity,
    )
}

pub(crate) fn invalidate_cached() -> Result<()> {
    let path = crate::config::mirror_dir().join("advice.json");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::anyhow!(
            "failed to invalidate advice cache {}: {}",
            path.display(),
            error
        )),
    }
}

pub(super) fn advice_cache_key(
    prompt: &str,
    system: &str,
    model_identity: &str,
    score_timestamp: DateTime<Utc>,
    long_cohort_identity: &str,
    recent_cohort_identity: &str,
) -> String {
    format!(
        "{}:{:016x}",
        CACHE_VERSION,
        stable_hash(&[
            prompt,
            system,
            model_identity,
            &score_timestamp.to_rfc3339(),
            long_cohort_identity,
            recent_cohort_identity,
        ])
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
    use super::super::policy::breadth_score;
    use super::*;
    use crate::score::Signal;
    use chrono::Duration;

    fn identity(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn cached(score_timestamp: DateTime<Utc>) -> CachedAdvice {
        CachedAdvice {
            advice: "current".into(),
            short: "current".into(),
            generated_at: Utc::now() - Duration::hours(2),
            cache_version: CACHE_VERSION.into(),
            cache_key: "key".into(),
            model_identity: "deterministic".into(),
            score_timestamp,
            policy_key: "deepen".into(),
            long_cohort_identity: identity('a'),
            recent_cohort_identity: identity('b'),
        }
    }

    #[test]
    fn display_cache_requires_exact_current_score_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        let current = Utc::now();
        std::fs::write(&path, serde_json::to_string(&cached(current)).unwrap()).unwrap();

        assert!(load_cached_matching(&path, None, Some(current))
            .unwrap()
            .is_some());
        assert!(
            load_cached_matching(&path, None, Some(current + Duration::seconds(1)))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn legacy_and_previous_semantics_are_not_displayed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("advice.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "advice": "legacy",
                "generated_at": Utc::now(),
                "cache_version": "advice-score-v4"
            })
            .to_string(),
        )
        .unwrap();
        assert!(load_cached_matching(&path, None, Some(Utc::now()))
            .unwrap()
            .is_none());
    }

    #[test]
    fn deterministic_cache_binds_policy_score_and_both_cohorts() {
        let long_term = breadth_score(14.4, Signal::Yellow, 35.0, Signal::Red);
        let recent = breadth_score(29.2, Signal::Green, 48.0, Signal::Red);
        let timestamp = Utc::now();
        let policy = portfolio_policy(&long_term, &recent).unwrap();
        let key = advice_cache_key(
            policy.mode.response_key(),
            "system",
            "model",
            timestamp,
            &identity('a'),
            &identity('b'),
        );
        assert_ne!(
            key,
            advice_cache_key(
                policy.mode.response_key(),
                "system",
                "model",
                timestamp,
                &identity('c'),
                &identity('b')
            )
        );
    }
}
