use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

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

    fn one(result: CommandResult) -> Self {
        Self {
            responses: RefCell::new(VecDeque::from([result])),
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

fn sessions(count: usize, summaries: Vec<Value>) -> Value {
    sessions_with_latest(count, summaries, Value::Null)
}

fn sessions_with_latest(count: usize, summaries: Vec<Value>, latest: Value) -> Value {
    serde_json::json!({
        "since_epoch": null, "until_epoch": null, "project": null,
        "sample": 0, "latest": latest, "count": count, "sessions": summaries
    })
}

fn project_sessions(count: usize, summaries: Vec<Value>) -> Value {
    let mut envelope = sessions(count, summaries);
    envelope["project"] = Value::String("/repo".into());
    envelope
}

fn summary(message_count: i64) -> Value {
    serde_json::json!({
        "session_ref": "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331",
        "host": "codex-cli",
        "source_root": "local", "project": "/repo", "session_id": "s1",
        "first_epoch": 10, "last_epoch": 20, "message_count": message_count,
        "user_message_count": (message_count + 1) / 2,
        "assistant_message_count": message_count / 2,
        "content_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "user_message_samples": []
    })
}

fn message(id: i64, epoch: i64, role: &str) -> Value {
    serde_json::json!({
        "id": id, "role": role, "content": format!("m{id}"),
        "source": "transcript", "branch": null, "cwd": "/repo",
        "created_at_epoch": epoch
    })
}

fn page(messages: Vec<Value>, has_more: bool, cursor: Value) -> Value {
    serde_json::json!({
        "source_type": "raw_archive", "host": "codex-cli", "source_root": "local",
        "project": "/repo", "session_id": "s1",
        "order": "created_at_epoch_asc_id_asc", "limit": 2000,
        "count": messages.len(), "has_more": has_more,
        "next_cursor": cursor,
        "content_hash": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "messages": messages
    })
}

#[test]
fn document_content_resolves_exact_summary_and_reuses_full_snapshot_validation() {
    let runner = FakeRunner::json(vec![
        project_sessions(1, vec![summary(3)]),
        page(
            vec![message(1, 10, "user"), message(2, 10, "assistant")],
            true,
            Value::String("c1".into()),
        ),
        page(vec![message(3, 20, "user")], false, Value::Null),
    ]);
    let session_ref = summary(3)["session_ref"].as_str().unwrap().to_string();
    let content = document::load_document_content_with_runner(
        &runner,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .unwrap();

    assert_eq!(content, "User: m1\nAssistant: m2\nUser: m3\n");
    assert_eq!(
        runner.calls.borrow()[0],
        strings(&[
            "raw",
            "sessions",
            "--project",
            "/repo",
            "--sample",
            "0",
            "--json",
        ])
    );
    assert!(runner.calls.borrow()[1]
        .windows(2)
        .any(|args| { args == ["--host".to_string(), "codex-cli".to_string()] }));
}

#[test]
fn document_content_rejects_mismatched_project_envelope() {
    let mut envelope = project_sessions(1, vec![summary(1)]);
    envelope["project"] = Value::String("/other".into());
    let runner = FakeRunner::json(vec![envelope]);
    let session_ref = summary(1)["session_ref"].as_str().unwrap().to_string();

    let error = document::load_document_content_with_runner(
        &runner,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("document hydration must bind the envelope to the decoded project");

    assert!(error.to_string().contains("raw sessions project drift"));
}

#[test]
fn document_content_rejects_stored_hash_and_missing_page_drift() {
    let session_ref = summary(3)["session_ref"].as_str().unwrap().to_string();
    let missing = FakeRunner::json(vec![project_sessions(0, vec![])]);
    let error = document::load_document_content_with_runner(
        &missing,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must resolve the exact v2 reference");
    assert!(error.to_string().contains("no longer resolves"));

    let stale = FakeRunner::json(vec![project_sessions(1, vec![summary(3)])]);
    let error = document::load_document_content_with_runner(
        &stale,
        &session_ref,
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    )
    .expect_err("doc-show must bind the stored hash to the current summary");
    assert!(error
        .to_string()
        .contains("stored Remem snapshot hash drifted"));

    let incomplete = FakeRunner::json(vec![
        project_sessions(1, vec![summary(3)]),
        page(
            vec![message(1, 10, "user"), message(2, 20, "assistant")],
            false,
            Value::Null,
        ),
    ]);
    let error = document::load_document_content_with_runner(
        &incomplete,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must reject a missing final page");
    assert!(error.to_string().contains("message count drift"));
}

#[test]
fn document_content_rejects_duplicate_ids_and_cross_page_reordering() {
    let session_ref = summary(3)["session_ref"].as_str().unwrap().to_string();
    let duplicate = FakeRunner::json(vec![
        project_sessions(1, vec![summary(3)]),
        page(
            vec![message(1, 10, "user"), message(2, 10, "assistant")],
            true,
            Value::String("c1".into()),
        ),
        page(vec![message(2, 20, "user")], false, Value::Null),
    ]);
    let error = document::load_document_content_with_runner(
        &duplicate,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must reject duplicate message IDs across pages");
    assert!(error.to_string().contains("duplicate raw message id"));

    let reordered = FakeRunner::json(vec![
        project_sessions(1, vec![summary(3)]),
        page(
            vec![message(1, 10, "user"), message(2, 20, "assistant")],
            true,
            Value::String("c1".into()),
        ),
        page(vec![message(3, 19, "user")], false, Value::Null),
    ]);
    let error = document::load_document_content_with_runner(
        &reordered,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must reject cross-page ordering drift");
    assert!(error.to_string().contains("not strictly monotonic"));
}

#[test]
fn document_content_rejects_summary_role_and_epoch_drift() {
    let session_ref = summary(2)["session_ref"].as_str().unwrap().to_string();
    let role_drift = FakeRunner::json(vec![
        project_sessions(1, vec![summary(2)]),
        page(
            vec![message(1, 10, "user"), message(2, 20, "user")],
            false,
            Value::Null,
        ),
    ]);
    let error = document::load_document_content_with_runner(
        &role_drift,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must bind role totals to the summary");
    assert!(error.to_string().contains("role counts drifted"));

    let epoch_drift = FakeRunner::json(vec![
        project_sessions(1, vec![summary(2)]),
        page(
            vec![message(1, 11, "user"), message(2, 20, "assistant")],
            false,
            Value::Null,
        ),
    ]);
    let error = document::load_document_content_with_runner(
        &epoch_drift,
        &session_ref,
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect_err("doc-show must bind first and last epochs to the summary");
    assert!(error.to_string().contains("first_epoch drifted"));
}

#[test]
fn loads_multiple_pages_and_builds_stable_identity() {
    let runner = FakeRunner::json(vec![
        sessions(1, vec![summary(3)]),
        page(
            vec![message(1, 10, "user"), message(2, 10, "assistant")],
            true,
            Value::String("c1".into()),
        ),
        page(vec![message(3, 20, "user")], false, Value::Null),
    ]);
    let loaded = load_remem_sessions_with_runner(&runner).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].session.messages.len(), 3);
    assert_eq!(loaded[0].session.source, SessionSource::Codex);
    assert_eq!(loaded[0].first_epoch, 10);
    assert_eq!(
        loaded[0].stable_document_url(),
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331"
    );
    assert!(runner.calls.borrow()[2].ends_with(&["--cursor".into(), "c1".into()]));
}

#[test]
fn summary_load_does_not_fetch_messages() {
    let runner = FakeRunner::json(vec![sessions(1, vec![summary(3)])]);
    let loaded = load_remem_session_summaries_with_runner(&runner).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].message_count, 3);
    assert_eq!(
        loaded[0].stable_document_url(),
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f7265706f/7331"
    );
    assert_eq!(runner.calls.borrow().len(), 1);
}

#[test]
fn legacy_identity_uniqueness_uses_the_complete_summary_set() {
    let first = summary(1);
    let mut second = summary(1);
    second["project"] = Value::String("/other".into());
    second["session_ref"] = Value::String(
        "remem://raw-session/v2/636f6465782d636c69/6c6f63616c/2f6f74686572/7331".into(),
    );
    let runner = FakeRunner::json(vec![sessions(2, vec![first, second])]);

    let loaded = load_remem_session_summaries_with_runner(&runner).unwrap();

    assert_eq!(loaded.len(), 2);
    assert!(loaded
        .iter()
        .all(|summary| !summary.legacy_identity_is_unique));
    assert_eq!(runner.calls.borrow().len(), 1);
}

#[test]
fn summary_load_always_requests_the_complete_remem_collection() {
    let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)])]);
    let loaded = load_remem_session_summaries_with_runner(&runner).unwrap();

    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].legacy_identity_is_unique);
    assert_eq!(
        runner.calls.borrow()[0],
        strings(&["raw", "sessions", "--sample", "0", "--json"])
    );

    let bounded = FakeRunner::json(vec![sessions_with_latest(
        1,
        vec![summary(1)],
        Value::from(1),
    )]);
    let error = load_remem_session_summaries_with_runner(&bounded)
        .expect_err("Refine must reject a silently bounded summary collection");
    assert!(error
        .to_string()
        .contains("unexpectedly applied latest bound"));

    for (field, value, expected_error) in [
        (
            "project",
            Value::String("/repo".into()),
            "unexpectedly applied project filter",
        ),
        (
            "since_epoch",
            Value::from(10),
            "unexpectedly applied since_epoch bound",
        ),
        (
            "until_epoch",
            Value::from(20),
            "unexpectedly applied until_epoch bound",
        ),
    ] {
        let mut filtered = sessions(1, vec![summary(1)]);
        filtered[field] = value;
        let runner = FakeRunner::json(vec![filtered]);
        let error = load_remem_session_summaries_with_runner(&runner)
            .expect_err("Refine must reject a silently filtered summary collection");
        assert!(error.to_string().contains(expected_error));
    }
}

#[test]
fn same_selector_from_two_hosts_is_preserved_without_legacy_guessing() {
    let codex = summary(1);
    let mut claude = summary(1);
    claude["host"] = Value::String("claude-code".into());
    claude["session_ref"] = Value::String(
        "remem://raw-session/v2/636c617564652d636f6465/6c6f63616c/2f7265706f/7331".into(),
    );
    let runner = FakeRunner::json(vec![sessions(2, vec![codex, claude])]);

    let loaded = load_remem_session_summaries_with_runner(&runner).unwrap();

    assert_eq!(loaded.len(), 2);
    assert_ne!(loaded[0].host, loaded[1].host);
    assert!(loaded
        .iter()
        .all(|summary| !summary.legacy_identity_is_unique));
}

#[test]
fn rejects_session_ref_that_does_not_encode_declared_selector() {
    let mut mismatched = summary(1);
    mismatched["project"] = Value::String("/other".into());
    let runner = FakeRunner::json(vec![sessions(1, vec![mismatched])]);

    let error = load_remem_session_summaries_with_runner(&runner)
        .expect_err("selector drift must fail closed");

    assert!(error
        .to_string()
        .contains("does not encode its declared selector"));
}

#[test]
fn reports_nonzero_and_invalid_json() {
    let failed = FakeRunner::one(CommandResult {
        success: false,
        code: Some(7),
        stdout: Vec::new(),
        stderr: b"locked".to_vec(),
    });
    assert!(load_remem_sessions_with_runner(&failed)
        .unwrap_err()
        .to_string()
        .contains("status Some(7)"));
    let invalid = FakeRunner::one(CommandResult {
        success: true,
        code: Some(0),
        stdout: b"not-json".to_vec(),
        stderr: Vec::new(),
    });
    assert!(load_remem_sessions_with_runner(&invalid).is_err());
}

#[test]
fn only_missing_launch_errors_are_fallback_candidates() {
    let missing = Err::<(), _>(io::Error::new(io::ErrorKind::NotFound, "remem"))
        .with_context(|| "run remem provider binary \"remem\"")
        .expect_err("the wrapped launch error should be preserved");
    assert!(is_missing_remem_executable(&missing));

    let denied = Err::<(), _>(io::Error::new(io::ErrorKind::PermissionDenied, "remem"))
        .with_context(|| "run remem provider binary \"remem\"")
        .expect_err("the wrapped launch error should be preserved");
    assert!(!is_missing_remem_executable(&denied));

    let contract = anyhow::anyhow!("raw sessions contract drift");
    assert!(!is_missing_remem_executable(&contract));
}

#[test]
fn rejects_missing_fields_and_count_drift() {
    let missing = FakeRunner::json(vec![serde_json::json!({"count": 0, "sessions": []})]);
    assert!(load_remem_sessions_with_runner(&missing).is_err());
    let drift = FakeRunner::json(vec![sessions(2, vec![summary(1)])]);
    assert!(load_remem_sessions_with_runner(&drift)
        .unwrap_err()
        .to_string()
        .contains("count mismatch"));
}

#[test]
fn rejects_selector_and_order_drift() {
    let mut selector = page(vec![message(1, 10, "user")], false, Value::Null);
    selector["project"] = Value::String("/other".into());
    let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)]), selector]);
    assert!(load_remem_sessions_with_runner(&runner).is_err());

    let mut order = page(vec![message(1, 10, "user")], false, Value::Null);
    order["order"] = Value::String("id_asc".into());
    let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)]), order]);
    assert!(load_remem_sessions_with_runner(&runner).is_err());
}

#[test]
fn rejects_snapshot_hash_drift_between_summary_and_messages() {
    let mut drifted = page(vec![message(1, 10, "user")], false, Value::Null);
    drifted["content_hash"] = Value::String(
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
    );
    let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)]), drifted]);

    let error = load_remem_sessions_with_runner(&runner)
        .expect_err("summary/messages snapshot mismatch must fail closed");
    assert!(error.to_string().contains("snapshot hash drifted"));
}

#[test]
fn rejects_cursor_stall_and_message_count_drift() {
    let runner = FakeRunner::json(vec![
        sessions(1, vec![summary(2)]),
        page(
            vec![message(1, 10, "user")],
            true,
            Value::String("same".into()),
        ),
        page(
            vec![message(2, 20, "assistant")],
            true,
            Value::String("same".into()),
        ),
    ]);
    assert!(load_remem_sessions_with_runner(&runner)
        .unwrap_err()
        .to_string()
        .contains("did not progress"));

    let runner = FakeRunner::json(vec![
        sessions(1, vec![summary(2)]),
        page(vec![message(1, 10, "user")], false, Value::Null),
    ]);
    assert!(load_remem_sessions_with_runner(&runner)
        .unwrap_err()
        .to_string()
        .contains("message count drift"));
}

#[test]
fn rejects_summary_role_and_epoch_drift() {
    let role_drift = FakeRunner::json(vec![
        sessions(1, vec![summary(2)]),
        page(
            vec![message(1, 10, "user"), message(2, 20, "user")],
            false,
            Value::Null,
        ),
    ]);
    assert!(load_remem_sessions_with_runner(&role_drift)
        .unwrap_err()
        .to_string()
        .contains("role counts drifted"));

    let epoch_drift = FakeRunner::json(vec![
        sessions(1, vec![summary(2)]),
        page(
            vec![message(1, 11, "user"), message(2, 20, "assistant")],
            false,
            Value::Null,
        ),
    ]);
    assert!(load_remem_sessions_with_runner(&epoch_drift)
        .unwrap_err()
        .to_string()
        .contains("first_epoch drifted"));
}

#[test]
fn empty_selection_is_success_and_unknown_roles_fail() {
    let empty = FakeRunner::json(vec![sessions(0, vec![])]);
    assert!(load_remem_sessions_with_runner(&empty).unwrap().is_empty());
    let runner = FakeRunner::json(vec![
        sessions(1, vec![summary(1)]),
        page(vec![message(1, 10, "system")], false, Value::Null),
    ]);
    assert!(load_remem_sessions_with_runner(&runner)
        .unwrap_err()
        .to_string()
        .contains("unsupported raw message role"));
}
