//! Commit intent comes from linked source messages, never Git metadata alone.
use super::query::QueryError;
use crate::state::AppState;
use refine_core::infra::ItemDto;
use refine_core::knowledge::SessionProjectionRevision;
use refine_core::session::{load_commit_context, CommitContext};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct CommitQuery {
    pub project: String,
    /// A commit SHA or a public GitHub PR URL.
    pub reference: String,
}

#[derive(Debug, Serialize)]
pub struct CommitProjection {
    pub session_ref: String,
    pub document_id: String,
    pub source_version: Option<String>,
    pub items: Vec<ItemDto>,
    pub evidence: serde_json::Value,
    pub history: Vec<SessionProjectionRevision>,
}

#[derive(Debug, Serialize)]
pub struct CommitResult {
    pub commits: Vec<CommitContext>,
    pub projections: Vec<CommitProjection>,
}

fn pr_selector(reference: &str) -> Result<Option<(&str, &str, u64)>, QueryError> {
    if !reference.starts_with("https://") {
        return Ok(None);
    }
    let parts = reference
        .strip_prefix("https://github.com/")
        .ok_or_else(|| QueryError::BadRequest("Use a SHA or a GitHub PR URL".into()))?
        .trim_end_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    if parts.len() != 4
        || parts[2] != "pull"
        || parts[..2].iter().any(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err(QueryError::BadRequest(
            "Expected https://github.com/owner/repo/pull/number".into(),
        ));
    }
    let number = parts[3]
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| QueryError::BadRequest("Invalid pull request number".into()))?;
    Ok(Some((parts[0], parts[1], number)))
}

async fn resolve_shas(reference: &str) -> Result<Vec<String>, QueryError> {
    let Some((owner, repo, number)) = pr_selector(reference)? else {
        if !(7..=64).contains(&reference.len()) || !reference.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(QueryError::BadRequest(
                "Expected a 7–64 digit hexadecimal SHA or GitHub PR URL".into(),
            ));
        }
        return Ok(vec![reference.to_ascii_lowercase()]);
    };
    #[derive(Deserialize)]
    struct Commit {
        sha: String,
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("refine-commit-context")
        .build()
        .map_err(|e| QueryError::Internal(e.to_string()))?;
    let mut shas = Vec::new();
    // GitHub's PR endpoint returns at most 250 commits. Refuse a truncated PR.
    for page in 1..=3 {
        let response = client.get(format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}/commits?per_page=100&page={page}"))
            .send().await.map_err(|e| QueryError::Internal(format!("GitHub PR lookup failed: {e}")))?;
        if !response.status().is_success() {
            return Err(QueryError::Internal(format!(
                "GitHub PR lookup returned HTTP {}",
                response.status()
            )));
        }
        let commits = response
            .json::<Vec<Commit>>()
            .await
            .map_err(|e| QueryError::Internal(format!("Invalid GitHub commit response: {e}")))?;
        let count = commits.len();
        shas.extend(commits.into_iter().map(|commit| commit.sha));
        if count < 100 {
            break;
        }
    }
    if shas.len() >= 250 {
        return Err(QueryError::BadRequest("PRs with 250 or more commits must be queried by individual SHA; the API cannot prove completeness".into()));
    }
    Ok(shas)
}

pub async fn lookup_commit(
    state: Arc<AppState>,
    query: CommitQuery,
) -> Result<CommitResult, QueryError> {
    if query.project.trim().is_empty() {
        return Err(QueryError::BadRequest("Project is required".into()));
    }
    let shas = resolve_shas(query.reference.trim()).await?;
    let mut commits = Vec::new();
    for sha in shas {
        let project = query.project.clone();
        commits.extend(
            tokio::task::spawn_blocking(move || load_commit_context(&project, &sha))
                .await
                .map_err(|e| QueryError::Internal(format!("Commit lookup task failed: {e}")))?
                .map_err(|e| QueryError::Internal(e.to_string()))?,
        );
    }
    let mut projections = Vec::new();
    for session in commits.iter().flat_map(|commit| &commit.sessions) {
        let Some(session_ref) = &session.session_ref else {
            continue;
        };
        if projections
            .iter()
            .any(|p: &CommitProjection| &p.session_ref == session_ref)
        {
            continue;
        }
        let Some(context) = state
            .doc_store
            .find_session_projection_context(session_ref)
            .await
            .map_err(|e| QueryError::Internal(e.to_string()))?
        else {
            continue;
        };
        projections.push(CommitProjection {
            session_ref: session_ref.clone(),
            document_id: context.document.id().to_string(),
            source_version: context.document.source_version().map(str::to_string),
            items: context.items.iter().map(ItemDto::from).collect(),
            evidence: context.evidence,
            history: context.history,
        });
    }
    Ok(CommitResult {
        commits,
        projections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pr_urls_cannot_redirect_or_choose_an_arbitrary_host() {
        assert_eq!(
            pr_selector("https://github.com/owner/repo/pull/12").unwrap(),
            Some(("owner", "repo", 12))
        );
        for url in [
            "https://localhost/pull/1",
            "https://github.com/owner/repo/pull/0",
            "https://github.com/owner/repo/pull/1?x",
            "https://github.com/owner/repo/pull/1/commits",
        ] {
            assert!(pr_selector(url).is_err());
        }
    }
    #[tokio::test]
    async fn malformed_references_fail_before_network() {
        for input in ["", "not-a-sha", "--help", "abcdef"] {
            assert!(resolve_shas(input).await.is_err());
        }
        assert_eq!(resolve_shas("ABCDEF1").await.unwrap(), vec!["abcdef1"]);
    }
}
