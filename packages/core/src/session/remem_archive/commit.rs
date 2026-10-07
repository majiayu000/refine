use super::{
    load_one_session, read_session_summaries, run_json, strings, MessageRole, ProcessRunner, Runner,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// A commit and its source discussions. An empty result means no commit was found.
#[derive(Debug, Serialize, Deserialize)]
pub struct CommitContext {
    pub sha: String,
    pub project: String,
    pub message: Option<String>,
    pub sessions: Vec<CommitDiscussion>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommitDiscussion {
    pub session_id: String,
    pub source: String,
    pub session_ref: Option<String>,
    pub content_hash: Option<String>,
    /// `verified` confirms the successful commit evidence and raw-session identity,
    /// not that a particular message explains the change. Otherwise `unknown`.
    pub status: String,
    pub messages: Vec<CommitMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommitMessage {
    pub id: i64,
    pub role: MessageRole,
    pub content: String,
    pub created_at_epoch: i64,
}

#[derive(Deserialize)]
struct CommitLookup {
    git: CommitMetadata,
    sessions: Vec<CommitLink>,
}

#[derive(Deserialize)]
struct CommitMetadata {
    sha: String,
    project: String,
    message: Option<String>,
}

#[derive(Deserialize)]
struct CommitLink {
    session_id: String,
    source: String,
}

pub fn load_commit_context(project: &str, sha: &str) -> Result<Vec<CommitContext>> {
    load_commit_context_with_runner(&ProcessRunner, project, sha)
}

fn load_commit_context_with_runner<R: Runner>(
    runner: &R,
    project: &str,
    sha: &str,
) -> Result<Vec<CommitContext>> {
    // Application callers validate the human selector once before dispatch.
    let needle = sha.to_ascii_lowercase();
    let lookups: Vec<CommitLookup> = run_json(
        runner,
        &strings(&["commit", "show", &needle, "--project", project, "--json"]),
        "commit show",
    )?;
    // Validate the query boundary before fetching any source content.
    for lookup in &lookups {
        ensure!(lookup.git.project == project, "commit show project drift");
        ensure!(
            (7..=64).contains(&lookup.git.sha.len())
                && lookup.git.sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                && lookup.git.sha.to_ascii_lowercase().starts_with(&needle),
            "commit show SHA drift"
        );
    }
    let summaries = if lookups.iter().any(|lookup| {
        lookup
            .sessions
            .iter()
            .any(|link| link.source == "capture_git_evidence")
    }) {
        read_session_summaries(
            runner,
            &strings(&[
                "raw",
                "sessions",
                "--project",
                project,
                "--sample",
                "0",
                "--json",
            ]),
            Some(project),
            0,
        )?
    } else {
        Vec::new()
    };
    lookups
        .into_iter()
        .map(|lookup| {
            let sessions = lookup
                .sessions
                .into_iter()
                .map(|link| {
                    let mut discussion = CommitDiscussion {
                        session_id: link.session_id,
                        source: link.source,
                        session_ref: None,
                        content_hash: None,
                        status: "unknown".into(),
                        messages: Vec::new(),
                    };
                    if discussion.source == "capture_git_evidence" {
                        let mut candidates = summaries.iter().filter(|summary| {
                            summary.project == project
                                && summary.session_id == discussion.session_id
                        });
                        if let Some(summary) = candidates.next() {
                            // Commit links do not identify host or source_root. Never guess them.
                            if candidates.next().is_none() {
                                let loaded = load_one_session(runner, summary.clone())?;
                                discussion.messages = loaded
                                    .session
                                    .messages
                                    .into_iter()
                                    .map(|message| {
                                        let provenance = message.provenance.context(
                                            "commit source message has no raw provenance",
                                        )?;
                                        Ok(CommitMessage {
                                            id: provenance.id,
                                            role: message.role,
                                            content: message.content,
                                            created_at_epoch: provenance.event_time.timestamp(),
                                        })
                                    })
                                    .collect::<Result<Vec<_>>>()?;
                                discussion.session_ref = Some(summary.session_ref.clone());
                                discussion.content_hash = Some(summary.content_hash.clone());
                                discussion.status = "verified".into();
                            }
                        }
                    }
                    Ok(discussion)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CommitContext {
                sha: lookup.git.sha,
                project: lookup.git.project,
                message: lookup.git.message,
                sessions,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::CommandResult;
    use super::*;
    use serde_json::{json, Value};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    const SHA: &str = "abcdef1234567890abcdef1234567890abcdef12";
    const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    struct FakeRunner {
        responses: RefCell<VecDeque<CommandResult>>,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl FakeRunner {
        fn json(values: Vec<Value>) -> Self {
            Self {
                responses: RefCell::new(
                    values
                        .into_iter()
                        .map(|value| CommandResult {
                            success: true,
                            code: Some(0),
                            stdout: serde_json::to_vec(&value).unwrap(),
                            stderr: Vec::new(),
                        })
                        .collect(),
                ),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Runner for FakeRunner {
        fn run(&self, args: &[String]) -> Result<CommandResult> {
            self.calls.borrow_mut().push(args.to_vec());
            self.responses
                .borrow_mut()
                .pop_front()
                .context("unexpected fake remem invocation")
        }
    }

    fn commit(source: &str) -> Value {
        json!([{"git": {"sha": SHA, "project": "/repo", "message": "Keep the decision"},
            "sessions": [{"session_id": "s1", "source": source}]}])
    }

    fn summary(root: &str) -> Value {
        json!({
            "session_ref": format!("remem://raw-session/v2/636f6465782d636c69/{}/2f7265706f/7331", super::super::hex_component(root)),
            "host": "codex-cli", "source_root": root, "project": "/repo", "session_id": "s1",
            "session_mode": "interactive", "first_epoch": 10, "last_epoch": 20,
            "message_count": 2, "user_message_count": 1, "assistant_message_count": 1,
            "content_hash": HASH, "user_message_samples": []
        })
    }

    fn summaries(rows: Vec<Value>) -> Value {
        json!({"since_epoch": null, "until_epoch": null, "project": "/repo", "sample": 0,
            "latest": null, "count": rows.len(), "sessions": rows})
    }

    fn page() -> Value {
        json!({
            "source_type": "raw_archive", "host": "codex-cli", "source_root": "local",
            "project": "/repo", "session_id": "s1", "order": "created_at_epoch_asc_id_asc",
            "limit": 2000, "count": 2, "has_more": false, "next_cursor": null, "content_hash": HASH,
            "messages": [
                {"id": 91, "role": "user", "content": "Why this approach?\nKeep my exact words.", "source": "transcript", "branch": null, "cwd": "/repo", "created_at_epoch": 10},
                {"id": 105, "role": "assistant", "content": "The alternative failed.", "source": "transcript", "branch": null, "cwd": "/repo", "created_at_epoch": 20}
            ]
        })
    }

    #[test]
    fn verified_commit_preserves_quotes_ids_roles_and_times() {
        let runner = FakeRunner::json(vec![
            commit("capture_git_evidence"),
            summaries(vec![summary("local")]),
            page(),
        ]);
        let contexts = load_commit_context_with_runner(&runner, "/repo", "ABCDEF1").unwrap();
        let discussion = &contexts[0].sessions[0];
        assert_eq!(discussion.status, "verified");
        assert_eq!(discussion.content_hash.as_deref(), Some(HASH));
        assert_eq!(
            discussion.session_ref.as_deref(),
            summary("local")["session_ref"].as_str()
        );
        assert_eq!(discussion.messages[0].id, 91);
        assert_eq!(discussion.messages[0].role, MessageRole::User);
        assert_eq!(
            discussion.messages[0].content,
            "Why this approach?\nKeep my exact words."
        );
        assert_eq!(discussion.messages[0].created_at_epoch, 10);
        assert_eq!(discussion.messages[1].id, 105);
        assert_eq!(discussion.messages[1].role, MessageRole::Assistant);
        assert_eq!(discussion.messages[1].created_at_epoch, 20);
        assert_eq!(
            runner.calls.borrow()[0],
            strings(&["commit", "show", "abcdef1", "--project", "/repo", "--json"])
        );
        assert_eq!(
            runner.calls.borrow()[2],
            strings(&[
                "raw",
                "messages",
                "--host",
                "codex-cli",
                "--source-root",
                "local",
                "--project",
                "/repo",
                "--session-id",
                "s1",
                "--limit",
                "2000",
                "--json"
            ])
        );
    }

    #[test]
    fn weak_links_never_claim_successful_commit_or_fetch_raw_content() {
        for source in ["git_metadata", "observations", "future-source"] {
            let runner = FakeRunner::json(vec![commit(source)]);
            let contexts = load_commit_context_with_runner(&runner, "/repo", SHA).unwrap();
            let discussion = &contexts[0].sessions[0];
            assert_eq!(discussion.status, "unknown");
            assert!(discussion.messages.is_empty());
            assert!(discussion.session_ref.is_none());
            assert_eq!(runner.calls.borrow().len(), 1);
        }
    }

    #[test]
    fn absent_or_ambiguous_raw_identity_remains_unknown() {
        for rows in [vec![], vec![summary("local"), summary("remote")]] {
            let runner = FakeRunner::json(vec![commit("capture_git_evidence"), summaries(rows)]);
            let contexts = load_commit_context_with_runner(&runner, "/repo", SHA).unwrap();
            assert_eq!(contexts[0].sessions[0].status, "unknown");
            assert!(contexts[0].sessions[0].messages.is_empty());
            assert_eq!(runner.calls.borrow().len(), 2);
        }
    }

    #[test]
    fn absent_commit_or_links_do_not_fetch_raw_sessions() {
        for value in [
            json!([]),
            json!([{"git": {"sha": SHA, "project": "/repo", "message": null}, "sessions": []}]),
        ] {
            let runner = FakeRunner::json(vec![value]);
            let contexts = load_commit_context_with_runner(&runner, "/repo", SHA).unwrap();
            assert!(contexts.is_empty() || contexts[0].sessions.is_empty());
            assert_eq!(runner.calls.borrow().len(), 1);
        }
    }

    #[test]
    fn hydration_contract_failures_propagate_instead_of_unknown() {
        for field in ["content_hash", "project", "order"] {
            let mut changed = page();
            changed[field] = json!("drift");
            let runner = FakeRunner::json(vec![
                commit("capture_git_evidence"),
                summaries(vec![summary("local")]),
                changed,
            ]);
            assert!(load_commit_context_with_runner(&runner, "/repo", SHA).is_err());
        }
        let mut partial = page();
        partial["messages"].as_array_mut().unwrap().pop();
        partial["count"] = json!(1);
        let runner = FakeRunner::json(vec![
            commit("capture_git_evidence"),
            summaries(vec![summary("local")]),
            partial,
        ]);
        assert!(load_commit_context_with_runner(&runner, "/repo", SHA)
            .unwrap_err()
            .to_string()
            .contains("message count drift"));
    }

    #[test]
    fn provider_failure_is_an_error() {
        let runner = FakeRunner::json(vec![
            commit("capture_git_evidence"),
            summaries(vec![summary("local")]),
        ]);
        runner.responses.borrow_mut().push_back(CommandResult {
            success: false,
            code: Some(2),
            stdout: Vec::new(),
            stderr: b"raw unavailable".to_vec(),
        });
        assert!(load_commit_context_with_runner(&runner, "/repo", SHA)
            .unwrap_err()
            .to_string()
            .contains("raw messages failed with status Some(2): raw unavailable"));
    }

    #[test]
    fn provider_boundary_rejects_sha_and_project_drift() {
        for (field, value) in [("project", "/other"), ("sha", "1234567")] {
            let mut changed = commit("capture_git_evidence");
            changed[0]["git"][field] = json!(value);
            let runner = FakeRunner::json(vec![changed]);
            assert!(load_commit_context_with_runner(&runner, "/repo", SHA).is_err());
            assert_eq!(runner.calls.borrow().len(), 1);
        }
    }
}
