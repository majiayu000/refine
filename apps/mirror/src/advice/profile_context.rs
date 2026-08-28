use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SCHEMA_VERSION: u32 = 3;
const SOURCE_REVISION: &str = "mirror-profile-context-v3-project-identity";
const COHORT_RELATION: &str = "exact-source-snapshot";
const STALE_AFTER_DAYS: i64 = 14;

#[derive(Debug, Serialize, Deserialize)]
struct ProfileContext {
    schema_version: u32,
    source_revision: String,
    generated_at: DateTime<Utc>,
    window: String,
    cohort_identity: String,
    cohort_relation: String,
    summary: String,
}

pub(crate) fn validate_cohort_identity(identity: &str, label: &str) -> Result<()> {
    let Some(digest) = identity.strip_prefix("sha256:") else {
        anyhow::bail!("{label} cohort identity must use sha256:<64hex>");
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        anyhow::bail!("{label} cohort identity must use sha256:<64hex>");
    }
    Ok(())
}

pub(crate) fn save_profile_context(
    summary: &str,
    window: &str,
    cohort_identity: &str,
) -> Result<()> {
    let dir = crate::config::ensure_mirror_dir()?;
    save_profile_context_to_path(
        &dir.join("profile-summary.json"),
        summary,
        window,
        cohort_identity,
        Utc::now(),
    )
}

pub(crate) fn save_profile_context_to_path(
    path: &Path,
    summary: &str,
    window: &str,
    cohort_identity: &str,
    generated_at: DateTime<Utc>,
) -> Result<()> {
    validate_cohort_identity(cohort_identity, "profile")?;
    let context = ProfileContext {
        schema_version: SCHEMA_VERSION,
        source_revision: SOURCE_REVISION.to_string(),
        generated_at,
        window: window.to_string(),
        cohort_identity: cohort_identity.to_string(),
        cohort_relation: COHORT_RELATION.to_string(),
        summary: summary.to_string(),
    };
    let json = serde_json::to_string_pretty(&context)?;
    std::fs::write(path, json).context("failed to write versioned profile summary")?;
    Ok(())
}

pub(crate) fn load_profile_context_from_path(
    path: &Path,
    now: DateTime<Utc>,
    expected_cohort_identity: &str,
) -> Result<Option<String>> {
    validate_cohort_identity(expected_cohort_identity, "expected advice")?;
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
    if context.schema_version != SCHEMA_VERSION {
        anyhow::bail!(
            "unsupported profile context schema {} in {}; expected {}",
            context.schema_version,
            path.display(),
            SCHEMA_VERSION
        );
    }
    if context.source_revision != SOURCE_REVISION {
        anyhow::bail!(
            "unsupported profile context source revision '{}' in {}; expected '{}'",
            context.source_revision,
            path.display(),
            SOURCE_REVISION
        );
    }
    if context.cohort_relation != COHORT_RELATION {
        anyhow::bail!(
            "unsupported profile cohort relation '{}' in {}; expected '{}'",
            context.cohort_relation,
            path.display(),
            COHORT_RELATION
        );
    }
    validate_cohort_identity(&context.cohort_identity, "profile")?;
    if context.generated_at > now {
        anyhow::bail!(
            "profile context {} has a future generated_at timestamp",
            path.display()
        );
    }
    if (now - context.generated_at).num_days() >= STALE_AFTER_DAYS
        || context.cohort_identity != expected_cohort_identity
    {
        return Ok(None);
    }
    if context.window.trim().is_empty() || context.summary.trim().is_empty() {
        anyhow::bail!(
            "profile context {} is missing window or summary",
            path.display()
        );
    }
    Ok(Some(format!(
        "generated_at={}\nwindow={}\nschema_version={}\nsource_revision={}\ncohort_identity={}\ncohort_relation=exact-match\n{}",
        context.generated_at.to_rfc3339(),
        context.window,
        context.schema_version,
        context.source_revision,
        context.cohort_identity,
        context.summary
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn identity(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    fn context(generated_at: DateTime<Utc>, cohort_identity: String) -> ProfileContext {
        ProfileContext {
            schema_version: SCHEMA_VERSION,
            source_revision: SOURCE_REVISION.into(),
            generated_at,
            window: "all eligible linked interactive observations".into(),
            cohort_identity,
            cohort_relation: COHORT_RELATION.into(),
            summary: "12 sessions, 2 projects".into(),
        }
    }

    #[test]
    fn fresh_exact_cohort_includes_reproducibility_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        let now = Utc::now();
        let expected = identity('a');
        std::fs::write(
            &path,
            serde_json::to_string(&context(now - Duration::days(2), expected.clone())).unwrap(),
        )
        .unwrap();

        let loaded = load_profile_context_from_path(&path, now, &expected)
            .unwrap()
            .expect("fresh exact profile context");
        assert!(loaded.contains("schema_version=3"));
        assert!(loaded.contains("source_revision=mirror-profile-context-v3-project-identity"));
        assert!(loaded.contains("cohort_relation=exact-match"));
    }

    #[test]
    fn writer_output_loads_for_the_same_rolling_90_day_cohort() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        let now = Utc::now();
        let cohort = identity('c');

        save_profile_context_to_path(
            &path,
            "12 sessions, 2 projects",
            "rolling 90 days (event time)",
            &cohort,
            now - Duration::hours(1),
        )
        .unwrap();

        let loaded = load_profile_context_from_path(&path, now, &cohort)
            .unwrap()
            .expect("writer output should load for its exact cohort");
        assert!(loaded.contains("window=rolling 90 days (event time)"));
        assert!(loaded.contains(&format!("cohort_identity={cohort}")));
        assert!(loaded.contains("cohort_relation=exact-match"));
    }

    #[test]
    fn stale_or_different_cohort_is_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        let now = Utc::now();
        let first = identity('a');
        std::fs::write(
            &path,
            serde_json::to_string(&context(now - Duration::days(14), first.clone())).unwrap(),
        )
        .unwrap();
        assert!(load_profile_context_from_path(&path, now, &first)
            .unwrap()
            .is_none());

        std::fs::write(
            &path,
            serde_json::to_string(&context(now - Duration::days(1), first)).unwrap(),
        )
        .unwrap();
        assert!(load_profile_context_from_path(&path, now, &identity('b'))
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_or_weak_identity_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile-summary.json");
        std::fs::write(&path, "legacy text without metadata").unwrap();
        assert!(
            load_profile_context_from_path(&path, Utc::now(), &identity('a'))
                .unwrap_err()
                .to_string()
                .contains("failed to parse profile context JSON")
        );

        let weak = context(Utc::now() - Duration::days(1), "sha256:test".into());
        std::fs::write(&path, serde_json::to_string(&weak).unwrap()).unwrap();
        assert!(
            load_profile_context_from_path(&path, Utc::now(), &identity('a'))
                .unwrap_err()
                .to_string()
                .contains("sha256:<64hex>")
        );
    }
}
