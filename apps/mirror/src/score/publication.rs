//! Serialize canonical publications, including snapshots without a score.
//! History retains daily measurements; this watermark also remembers empty
//! snapshots so a late writer cannot resurrect their invalidated output.
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use super::baseline::{compute_personal_trends, PersonalTrends};
use super::persistence::{
    load_recent_scores_for_scope_from_path, persist_score_to_path, write_lines_atomically,
};
use super::{compute_personal_baseline, ScoreResult, ScoreScope};

const WATERMARK_FILE: &str = "score-publication.json";
const WATERMARK_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
struct Publication {
    cutoff: DateTime<Utc>,
    available: bool,
}

#[derive(Serialize, Deserialize)]
struct CacheOwner {
    scope_key: String,
    #[serde(flatten)]
    publication: Publication,
}

#[derive(Serialize, Deserialize)]
struct Watermark {
    version: u32,
    scopes: BTreeMap<String, Publication>,
    owner: Option<CacheOwner>,
}

impl Watermark {
    fn load(directory: &Path) -> Result<Self> {
        let path = directory.join(WATERMARK_FILE);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let state: Self = serde_json::from_str(&content)
                    .with_context(|| format!("parse publication watermark {}", path.display()))?;
                anyhow::ensure!(
                    state.version == WATERMARK_VERSION,
                    "unsupported publication watermark version {}",
                    state.version
                );
                Ok(state)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Self::from_existing_history(directory)
            }
            Err(error) => {
                Err(error).with_context(|| format!("read publication watermark {}", path.display()))
            }
        }
    }

    fn from_existing_history(directory: &Path) -> Result<Self> {
        // Upgrade without changing history or trusting a timestamp alone to
        // identify the database that owns the single shared advice cache.
        let history = load_recent_scores_for_scope_from_path(
            &directory.join("scores.jsonl"),
            usize::MAX,
            None,
        )?;
        let mut scopes = BTreeMap::new();
        for score in &history {
            let scope = score.scope.as_ref().expect("canonical history has a scope");
            scopes.insert(
                scope_key(scope)?,
                Publication {
                    cutoff: score.timestamp,
                    available: true,
                },
            );
        }
        #[derive(Deserialize)]
        struct CacheIdentity {
            score_timestamp: DateTime<Utc>,
            long_cohort_identity: String,
        }
        // An unreadable, malformed, or unowned legacy cache is not evidence of
        // ownership. Its normal reader/remover still reports any I/O errors.
        let identity = std::fs::read_to_string(directory.join("advice.json"))
            .ok()
            .and_then(|content| serde_json::from_str::<CacheIdentity>(&content).ok());
        let mut owners = BTreeMap::new();
        if let Some(identity) = identity {
            for score in &history {
                let scope = score.scope.as_ref().expect("canonical history has a scope");
                let key = scope_key(scope)?;
                if score.timestamp == identity.score_timestamp
                    && scope.cohort_identity == identity.long_cohort_identity
                    && scopes[&key].cutoff == score.timestamp
                {
                    owners.insert(key, score.timestamp);
                }
            }
        }
        let owner = if owners.len() == 1 {
            owners
                .into_iter()
                .next()
                .map(|(scope_key, cutoff)| CacheOwner {
                    scope_key,
                    publication: Publication {
                        cutoff,
                        available: true,
                    },
                })
        } else {
            None
        };
        Ok(Self {
            version: WATERMARK_VERSION,
            scopes,
            owner,
        })
    }

    fn save(&self, directory: &Path) -> Result<()> {
        write_lines_atomically(
            &directory.join(WATERMARK_FILE),
            &[serde_json::to_string_pretty(self)?],
        )
        .context("persist publication watermark")
    }

    fn is_newer(&self, key: &str, cutoff: DateTime<Utc>) -> bool {
        self.scopes.get(key).is_none_or(|last| cutoff > last.cutoff)
    }
}

fn scope_key(scope: &ScoreScope) -> Result<String> {
    anyhow::ensure!(scope.is_canonical(), "invalid canonical publication scope");
    // Cohort, quality, and exact rolling bounds deliberately do not participate
    // in equality: otherwise a curation change would bypass the empty fence.
    Ok(serde_json::to_string(&scope.retention_key())?)
}

pub(crate) struct PublishedScore {
    pub trends: Option<PersonalTrends>,
    pub advice: Result<String>,
}

/// The supplied directory is shared by history, watermark, advice, and statusline.
/// Every production writer of these derived files goes through this lock.
pub(crate) fn publish_canonical_score(
    result: &ScoreResult,
    recent: &ScoreResult,
    recent_cohort_identity: &str,
    directory: &Path,
) -> Result<PublishedScore> {
    let scope = result
        .scope
        .as_ref()
        .context("canonical publication requires a score scope")?;
    let key = scope_key(scope)?;
    anyhow::ensure!(
        result.timestamp == scope.window_end,
        "invalid canonical publication timestamp"
    );
    with_publication_lock(directory, || {
        let mut state = Watermark::load(directory)?;
        let history_path = directory.join("scores.jsonl");
        let history = load_recent_scores_for_scope_from_path(&history_path, 365, Some(scope))?;
        let trends = compute_personal_baseline(&history, scope)
            .as_ref()
            .map(|baseline| compute_personal_trends(result, baseline));
        let local_view = || {
            crate::advice::portfolio_policy(result, recent)
                .map(|policy| crate::advice::deterministic_advice(&policy))
        };
        if !state.is_newer(&key, result.timestamp) {
            // A later valid or empty snapshot already owns this scope. The
            // requested view may render, but no historical/derived file changes.
            return Ok(PublishedScore {
                trends,
                advice: local_view(),
            });
        }
        let publication = Publication {
            cutoff: result.timestamp,
            available: true,
        };
        let owns_output = state
            .owner
            .as_ref()
            .is_none_or(|owner| result.timestamp > owner.publication.cutoff);
        state.scopes.insert(key.clone(), publication.clone());
        if owns_output {
            state.owner = Some(CacheOwner {
                scope_key: key,
                publication,
            });
        }
        // Persist the fence before any derived mutation. A failed write cannot
        // let an older process revive previous advice; a later cutoff can retry.
        state.save(directory)?;
        let retained = persist_score_to_path(&history_path, result)?;
        let advice = if retained && owns_output {
            match crate::advice::cache_current_deterministic(
                directory,
                result,
                recent,
                result.timestamp,
                &scope.cohort_identity,
                recent_cohort_identity,
            ) {
                Ok(advice) => Ok(advice),
                Err(error) => {
                    tracing::error!("portfolio advice failed: {}", error);
                    Err(match crate::advice::invalidate_cached(directory) {
                        Ok(()) => error,
                        Err(invalidation) => invalidation.context(format!(
                            "portfolio advice failed ({error}); stale advice cache also could not be invalidated"
                        )),
                    })
                }
            }
        } else {
            local_view()
        };
        if retained && owns_output {
            if let Err(error) =
                super::statusline::write_statusline(result, directory, trends.as_ref())
            {
                tracing::warn!("failed to write statusline.txt: {}", error);
            }
        }
        Ok(PublishedScore { trends, advice })
    })
}

pub(crate) fn invalidate_empty_score_cache(
    scope: Option<&ScoreScope>,
    directory: &Path,
) -> Result<()> {
    // An ad hoc display window says nothing about the canonical portfolio.
    let Some(scope) = scope else { return Ok(()) };
    let key = scope_key(scope)?;
    with_publication_lock(directory, || {
        let mut state = Watermark::load(directory)?;
        if !state.is_newer(&key, scope.window_end) {
            return Ok(());
        }
        let publication = Publication {
            cutoff: scope.window_end,
            available: false,
        };
        state.scopes.insert(key.clone(), publication.clone());
        let owns_output = state
            .owner
            .as_ref()
            .is_none_or(|owner| owner.scope_key == key);
        if owns_output {
            state.owner = Some(CacheOwner {
                scope_key: key,
                publication,
            });
        }
        // Even the first empty snapshot creates a watermark; no score is made
        // up just to carry its cutoff. Another database keeps its own cache.
        state.save(directory)?;
        if owns_output {
            for filename in ["advice.json", "statusline.txt"] {
                let path = directory.join(filename);
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(anyhow::anyhow!(
                            "failed to invalidate empty-score cache {}: {error}",
                            path.display()
                        ));
                    }
                }
            }
        }
        Ok(())
    })
}

pub(crate) fn cache_belongs_to_score(directory: &Path, score: &ScoreResult) -> Result<bool> {
    let Some(scope) = score.scope.as_ref() else {
        return Ok(false);
    };
    if !scope.is_canonical() || score.timestamp != scope.window_end {
        return Ok(false);
    }
    let key = scope_key(scope)?;
    let state = Watermark::load(directory)?;
    Ok(state.owner.as_ref().is_some_and(|owner| {
        owner.scope_key == key
            && owner.publication.available
            && owner.publication.cutoff == score.timestamp
            && state
                .scopes
                .get(&key)
                .is_some_and(|last| last.available && last.cutoff == score.timestamp)
    }))
}

fn with_publication_lock<T>(directory: &Path, work: impl FnOnce() -> Result<T>) -> Result<T> {
    std::fs::create_dir_all(directory).with_context(|| {
        format!(
            "create canonical publication directory {}",
            directory.display()
        )
    })?;
    let path = directory.join("score-publication.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open canonical publication lock {}", path.display()))?;
    fs2::FileExt::lock_exclusive(&lock).context("acquire canonical publication lock")?;
    let result = work();
    let unlocked = fs2::FileExt::unlock(&lock).context("release canonical publication lock");
    let value = result?;
    unlocked?;
    Ok(value)
}
