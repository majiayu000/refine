use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use refine_core::session::{MessageRole, Session, SessionMessage, SessionMeta, SessionSource};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::process::Command;

const RAW_SOURCE_TYPE: &str = "raw_archive";
const RAW_MESSAGE_ORDER: &str = "created_at_epoch_asc_id_asc";
const RAW_MESSAGE_LIMIT: &str = "2000";

#[derive(Debug)]
pub(crate) struct RememSession {
    pub source_root: String,
    pub project: String,
    pub session_id: String,
    pub first_epoch: i64,
    pub session: Session,
}

impl RememSession {
    pub(crate) fn stable_document_url(&self) -> String {
        format!(
            "remem-raw://v1/{}/{}/{}",
            hex_component(&self.source_root),
            hex_component(&self.project),
            hex_component(&self.session_id)
        )
    }
}

pub(crate) fn load_remem_session_summaries(
    limit: Option<usize>,
    latest: Option<usize>,
) -> Result<Vec<RememSessionSummary>> {
    load_remem_session_summaries_with_runner(&ProcessRunner, limit, latest)
}

pub(crate) fn load_remem_session(summary: RememSessionSummary) -> Result<RememSession> {
    load_one_session(&ProcessRunner, summary)
}

fn hex_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

struct CommandResult {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

trait Runner {
    fn run(&self, args: &[String]) -> Result<CommandResult>;
}

struct ProcessRunner;

pub(crate) fn is_missing_remem_executable(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
    })
}

impl Runner for ProcessRunner {
    fn run(&self, args: &[String]) -> Result<CommandResult> {
        let binary = std::env::var("REFINE_REMEM_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "remem".to_string());
        let output = Command::new(&binary)
            .args(args)
            .output()
            .with_context(|| format!("run remem provider binary {binary:?}"))?;
        Ok(CommandResult {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

#[derive(Debug, Deserialize)]
struct SessionsEnvelope {
    since_epoch: Value,
    until_epoch: Value,
    project: Value,
    sample: i64,
    count: usize,
    sessions: Vec<RememSessionSummary>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RememSessionSummary {
    pub source_root: String,
    pub project: String,
    pub session_id: String,
    pub first_epoch: i64,
    pub last_epoch: i64,
    pub message_count: i64,
    pub user_message_count: i64,
    pub assistant_message_count: i64,
    pub user_message_samples: Vec<String>,
    #[serde(skip)]
    pub legacy_identity_is_unique: bool,
}

impl RememSessionSummary {
    pub(crate) fn stable_document_url(&self) -> String {
        format!(
            "remem-raw://v1/{}/{}/{}",
            hex_component(&self.source_root),
            hex_component(&self.project),
            hex_component(&self.session_id)
        )
    }
}

#[derive(Debug, Deserialize)]
struct MessagesEnvelope {
    source_type: String,
    source_root: String,
    project: String,
    session_id: String,
    order: String,
    limit: i64,
    count: usize,
    has_more: bool,
    next_cursor: Value,
    messages: Vec<RawMessage>,
}

#[derive(Debug, Deserialize)]
struct RawMessage {
    id: i64,
    role: String,
    content: String,
    source: String,
    branch: Value,
    cwd: Value,
    created_at_epoch: i64,
}

fn load_remem_session_summaries_with_runner<R: Runner>(
    runner: &R,
    limit: Option<usize>,
    latest: Option<usize>,
) -> Result<Vec<RememSessionSummary>> {
    ensure!(
        limit.is_none() || latest.is_none(),
        "limit and latest cannot be used together"
    );
    let args = strings(&["raw", "sessions", "--sample", "0", "--json"]);
    let mut summaries = read_session_summaries(runner, &args)?;
    if latest.is_some() {
        summaries.sort_by(|left, right| {
            right
                .last_epoch
                .cmp(&left.last_epoch)
                .then_with(|| left.source_root.cmp(&right.source_root))
                .then_with(|| left.project.cmp(&right.project))
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
    }
    if let Some(max) = latest.or(limit) {
        summaries.truncate(max);
    }
    Ok(summaries)
}

#[cfg(test)]
fn load_remem_sessions_with_runner<R: Runner>(
    runner: &R,
    limit: Option<usize>,
    latest: Option<usize>,
) -> Result<Vec<RememSession>> {
    load_remem_session_summaries_with_runner(runner, limit, latest)?
        .into_iter()
        .map(|summary| load_one_session(runner, summary))
        .collect()
}

fn read_session_summaries<R: Runner>(
    runner: &R,
    args: &[String],
) -> Result<Vec<RememSessionSummary>> {
    let mut envelope: SessionsEnvelope = run_json(runner, args, "raw sessions")?;
    validate_nullable_i64(&envelope.since_epoch, "raw sessions since_epoch")?;
    validate_nullable_i64(&envelope.until_epoch, "raw sessions until_epoch")?;
    validate_nullable_string(&envelope.project, "raw sessions project")?;
    ensure!(envelope.sample == 0, "raw sessions sample drifted from 0");
    ensure!(
        envelope.count == envelope.sessions.len(),
        "raw sessions count mismatch: declared {}, received {}",
        envelope.count,
        envelope.sessions.len()
    );
    let mut tuples = HashSet::new();
    for summary in &envelope.sessions {
        ensure!(
            !summary.source_root.is_empty(),
            "raw session source_root is empty"
        );
        ensure!(!summary.project.is_empty(), "raw session project is empty");
        ensure!(
            !summary.session_id.is_empty(),
            "raw session session_id is empty"
        );
        ensure!(
            summary.first_epoch <= summary.last_epoch,
            "raw session epoch order is invalid"
        );
        ensure!(
            summary.message_count > 0,
            "raw session message_count must be positive"
        );
        ensure!(
            summary.user_message_count >= 0,
            "raw session user count is negative"
        );
        ensure!(
            summary.assistant_message_count >= 0,
            "raw session assistant count is negative"
        );
        ensure!(
            summary.user_message_count + summary.assistant_message_count == summary.message_count,
            "raw session role counts do not match message_count"
        );
        ensure!(
            summary.user_message_samples.is_empty(),
            "raw sessions returned samples for sample=0"
        );
        ensure!(
            tuples.insert((&summary.source_root, &summary.project, &summary.session_id)),
            "raw sessions returned a duplicate selector tuple"
        );
    }
    let mut identity_counts = HashMap::new();
    for summary in &envelope.sessions {
        *identity_counts
            .entry((summary.source_root.clone(), summary.session_id.clone()))
            .or_insert(0usize) += 1;
    }
    for summary in &mut envelope.sessions {
        summary.legacy_identity_is_unique = identity_counts
            .get(&(summary.source_root.clone(), summary.session_id.clone()))
            == Some(&1);
    }
    Ok(envelope.sessions)
}

fn load_one_session<R: Runner>(runner: &R, summary: RememSessionSummary) -> Result<RememSession> {
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut seen_ids = HashSet::new();
    let mut previous_key: Option<(i64, i64)> = None;
    let mut messages = Vec::new();
    let mut user_messages = 0_i64;
    let mut assistant_messages = 0_i64;
    let mut first_message_epoch = None;
    let mut last_message_epoch = None;
    loop {
        let args = message_args(&summary, cursor.as_deref());
        let envelope: MessagesEnvelope = run_json(runner, &args, "raw messages")?;
        validate_page(&summary, &envelope)?;
        ensure!(
            envelope.count == envelope.messages.len(),
            "raw messages count mismatch: declared {}, received {}",
            envelope.count,
            envelope.messages.len()
        );
        let next_cursor = validated_next_cursor(&envelope, cursor.as_deref(), &mut seen_cursors)?;
        for raw in envelope.messages {
            validate_nullable_string(&raw.branch, "raw message branch")?;
            validate_nullable_string(&raw.cwd, "raw message cwd")?;
            ensure!(!raw.source.is_empty(), "raw message source is empty");
            ensure!(
                seen_ids.insert(raw.id),
                "duplicate raw message id {}",
                raw.id
            );
            let key = (raw.created_at_epoch, raw.id);
            if let Some(previous) = previous_key {
                ensure!(
                    key > previous,
                    "raw message order is not strictly monotonic"
                );
            }
            previous_key = Some(key);
            first_message_epoch.get_or_insert(raw.created_at_epoch);
            last_message_epoch = Some(raw.created_at_epoch);
            let role = match raw.role.as_str() {
                "user" => {
                    user_messages += 1;
                    MessageRole::User
                }
                "assistant" => {
                    assistant_messages += 1;
                    MessageRole::Assistant
                }
                other => bail!("unsupported raw message role {other:?}"),
            };
            messages.push(SessionMessage {
                role,
                content: raw.content,
            });
        }
        cursor = next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    ensure!(
        messages.len() as i64 == summary.message_count,
        "raw session message count drift for ({:?}, {:?}, {:?}): expected {}, received {}",
        summary.source_root,
        summary.project,
        summary.session_id,
        summary.message_count,
        messages.len()
    );
    ensure!(
        user_messages == summary.user_message_count
            && assistant_messages == summary.assistant_message_count,
        "raw session role counts drifted from the session summary"
    );
    ensure!(
        first_message_epoch == Some(summary.first_epoch),
        "raw session first_epoch drifted from the session summary"
    );
    ensure!(
        last_message_epoch == Some(summary.last_epoch),
        "raw session last_epoch drifted from the session summary"
    );
    let started_at = DateTime::<Utc>::from_timestamp(summary.first_epoch, 0)
        .context("raw session first_epoch is outside chrono range")?;
    let mut result = RememSession {
        source_root: summary.source_root,
        project: summary.project,
        session_id: summary.session_id,
        first_epoch: summary.first_epoch,
        session: Session {
            source: SessionSource::RememRaw,
            file_path: PathBuf::new(),
            messages,
            meta: SessionMeta {
                project: None,
                model: None,
                started_at: Some(started_at),
            },
        },
    };
    result.session.meta.project = Some(result.project.clone());
    result.session.file_path = PathBuf::from(result.stable_document_url());
    Ok(result)
}

fn validate_page(summary: &RememSessionSummary, envelope: &MessagesEnvelope) -> Result<()> {
    ensure!(
        envelope.source_type == RAW_SOURCE_TYPE,
        "raw messages source_type drift"
    );
    ensure!(
        envelope.source_root == summary.source_root,
        "raw messages source_root drift"
    );
    ensure!(
        envelope.project == summary.project,
        "raw messages project drift"
    );
    ensure!(
        envelope.session_id == summary.session_id,
        "raw messages session_id drift"
    );
    ensure!(
        envelope.order == RAW_MESSAGE_ORDER,
        "raw messages order drift"
    );
    ensure!(envelope.limit == 2000, "raw messages limit drift");
    Ok(())
}

fn validated_next_cursor(
    envelope: &MessagesEnvelope,
    previous: Option<&str>,
    seen: &mut HashSet<String>,
) -> Result<Option<String>> {
    let cursor = value_as_optional_string(&envelope.next_cursor, "raw messages next_cursor")?;
    if envelope.has_more {
        let cursor = cursor.context("raw messages has_more=true without next_cursor")?;
        ensure!(!cursor.is_empty(), "raw messages next_cursor is empty");
        ensure!(
            !envelope.messages.is_empty(),
            "raw messages cursor made no row progress"
        );
        ensure!(
            previous != Some(cursor.as_str()),
            "raw messages cursor did not progress"
        );
        ensure!(seen.insert(cursor.clone()), "raw messages cursor repeated");
        Ok(Some(cursor))
    } else {
        ensure!(
            cursor.is_none(),
            "raw messages has_more=false with next_cursor"
        );
        Ok(None)
    }
}

fn message_args(summary: &RememSessionSummary, cursor: Option<&str>) -> Vec<String> {
    let mut args = strings(&[
        "raw",
        "messages",
        "--source-root",
        &summary.source_root,
        "--project",
        &summary.project,
        "--session-id",
        &summary.session_id,
        "--limit",
        RAW_MESSAGE_LIMIT,
        "--json",
    ]);
    if let Some(cursor) = cursor {
        args.push("--cursor".to_string());
        args.push(cursor.to_string());
    }
    args
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn run_json<T: for<'de> Deserialize<'de>, R: Runner>(
    runner: &R,
    args: &[String],
    operation: &str,
) -> Result<T> {
    let output = runner.run(args)?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr_excerpt = stderr.trim().chars().take(500).collect::<String>();
        bail!(
            "{operation} failed with status {:?}: {}",
            output.code,
            stderr_excerpt
        );
    }
    serde_json::from_slice(&output.stdout).with_context(|| format!("parse {operation} JSON"))
}

fn validate_nullable_i64(value: &Value, field: &str) -> Result<()> {
    ensure!(
        value.is_null() || value.as_i64().is_some(),
        "{field} has invalid type"
    );
    Ok(())
}

fn validate_nullable_string(value: &Value, field: &str) -> Result<()> {
    value_as_optional_string(value, field).map(|_| ())
}

fn value_as_optional_string(value: &Value, field: &str) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        _ => bail!("{field} has invalid type"),
    }
}

#[cfg(test)]
mod tests {
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
        serde_json::json!({
            "since_epoch": null, "until_epoch": null, "project": null,
            "sample": 0, "count": count, "sessions": summaries
        })
    }

    fn summary(message_count: i64) -> Value {
        serde_json::json!({
            "source_root": "local", "project": "/repo", "session_id": "s1",
            "first_epoch": 10, "last_epoch": 20, "message_count": message_count,
            "user_message_count": (message_count + 1) / 2,
            "assistant_message_count": message_count / 2,
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
            "source_type": "raw_archive", "source_root": "local",
            "project": "/repo", "session_id": "s1",
            "order": "created_at_epoch_asc_id_asc", "limit": 2000,
            "count": messages.len(), "has_more": has_more,
            "next_cursor": cursor, "messages": messages
        })
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
        let loaded = load_remem_sessions_with_runner(&runner, None, None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].session.messages.len(), 3);
        assert_eq!(loaded[0].session.source, SessionSource::RememRaw);
        assert_eq!(loaded[0].first_epoch, 10);
        assert_eq!(
            loaded[0].stable_document_url(),
            "remem-raw://v1/6c6f63616c/2f7265706f/7331"
        );
        assert!(runner.calls.borrow()[2].ends_with(&["--cursor".into(), "c1".into()]));
    }

    #[test]
    fn summary_load_does_not_fetch_messages() {
        let runner = FakeRunner::json(vec![sessions(1, vec![summary(3)])]);
        let loaded = load_remem_session_summaries_with_runner(&runner, None, None).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].message_count, 3);
        assert_eq!(
            loaded[0].stable_document_url(),
            "remem-raw://v1/6c6f63616c/2f7265706f/7331"
        );
        assert_eq!(runner.calls.borrow().len(), 1);
    }

    #[test]
    fn legacy_identity_uniqueness_is_computed_before_selection() {
        let first = summary(1);
        let mut second = summary(1);
        second["project"] = Value::String("/other".into());
        let runner = FakeRunner::json(vec![sessions(2, vec![first, second])]);

        let loaded = load_remem_session_summaries_with_runner(&runner, None, Some(1)).unwrap();

        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].legacy_identity_is_unique);
        assert_eq!(runner.calls.borrow().len(), 1);
    }

    #[test]
    fn reports_nonzero_and_invalid_json() {
        let failed = FakeRunner::one(CommandResult {
            success: false,
            code: Some(7),
            stdout: Vec::new(),
            stderr: b"locked".to_vec(),
        });
        assert!(load_remem_sessions_with_runner(&failed, None, None)
            .unwrap_err()
            .to_string()
            .contains("status Some(7)"));
        let invalid = FakeRunner::one(CommandResult {
            success: true,
            code: Some(0),
            stdout: b"not-json".to_vec(),
            stderr: Vec::new(),
        });
        assert!(load_remem_sessions_with_runner(&invalid, None, None).is_err());
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
        assert!(load_remem_sessions_with_runner(&missing, None, None).is_err());
        let drift = FakeRunner::json(vec![sessions(2, vec![summary(1)])]);
        assert!(load_remem_sessions_with_runner(&drift, None, None)
            .unwrap_err()
            .to_string()
            .contains("count mismatch"));
    }

    #[test]
    fn rejects_selector_and_order_drift() {
        let mut selector = page(vec![message(1, 10, "user")], false, Value::Null);
        selector["project"] = Value::String("/other".into());
        let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)]), selector]);
        assert!(load_remem_sessions_with_runner(&runner, None, None).is_err());

        let mut order = page(vec![message(1, 10, "user")], false, Value::Null);
        order["order"] = Value::String("id_asc".into());
        let runner = FakeRunner::json(vec![sessions(1, vec![summary(1)]), order]);
        assert!(load_remem_sessions_with_runner(&runner, None, None).is_err());
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
        assert!(load_remem_sessions_with_runner(&runner, None, None)
            .unwrap_err()
            .to_string()
            .contains("did not progress"));

        let runner = FakeRunner::json(vec![
            sessions(1, vec![summary(2)]),
            page(vec![message(1, 10, "user")], false, Value::Null),
        ]);
        assert!(load_remem_sessions_with_runner(&runner, None, None)
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
        assert!(load_remem_sessions_with_runner(&role_drift, None, None)
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
        assert!(load_remem_sessions_with_runner(&epoch_drift, None, None)
            .unwrap_err()
            .to_string()
            .contains("first_epoch drifted"));
    }

    #[test]
    fn empty_selection_is_success_and_unknown_roles_fail() {
        let empty = FakeRunner::json(vec![sessions(0, vec![])]);
        assert!(load_remem_sessions_with_runner(&empty, None, None)
            .unwrap()
            .is_empty());
        let runner = FakeRunner::json(vec![
            sessions(1, vec![summary(1)]),
            page(vec![message(1, 10, "system")], false, Value::Null),
        ]);
        assert!(load_remem_sessions_with_runner(&runner, None, None)
            .unwrap_err()
            .to_string()
            .contains("unsupported raw message role"));
    }
}
