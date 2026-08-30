use super::types::{MessageRole, Session, SessionMessage, SessionMeta, SessionMode, SessionSource};
use anyhow::{bail, ensure, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::process::Command;

const RAW_SOURCE_TYPE: &str = "raw_archive";
const RAW_MESSAGE_ORDER: &str = "created_at_epoch_asc_id_asc";
const RAW_MESSAGE_LIMIT: &str = "2000";

mod document;
pub use document::load_document_content as load_remem_document_content;

#[derive(Debug)]
pub struct RememSession {
    pub session_ref: String,
    pub source_root: String,
    pub project: String,
    pub session_id: String,
    pub first_epoch: i64,
    pub session: Session,
}

impl RememSession {
    pub fn stable_document_url(&self) -> String {
        self.session_ref.clone()
    }
}

pub fn load_remem_session_summaries() -> Result<Vec<RememSessionSummary>> {
    load_remem_session_summaries_with_runner(&ProcessRunner)
}

pub fn load_remem_session(summary: RememSessionSummary) -> Result<RememSession> {
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

pub fn is_missing_remem_executable(error: &anyhow::Error) -> bool {
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
    latest: Value,
    count: usize,
    sessions: Vec<RememSessionSummary>,
}

#[derive(Debug, Deserialize)]
pub struct RememSessionSummary {
    pub session_ref: String,
    pub host: String,
    pub session_mode: String,
    pub source_root: String,
    pub project: String,
    pub session_id: String,
    pub first_epoch: i64,
    pub last_epoch: i64,
    pub message_count: i64,
    pub user_message_count: i64,
    pub assistant_message_count: i64,
    pub content_hash: String,
    pub user_message_samples: Vec<String>,
    #[serde(skip)]
    pub legacy_identity_is_unique: bool,
}

impl RememSessionSummary {
    pub fn stable_document_url(&self) -> String {
        self.session_ref.clone()
    }

    pub fn projection_version(&self) -> String {
        format!("{}:{}", self.content_hash, self.session_mode)
    }

    pub fn session_source(&self) -> Result<SessionSource> {
        match self.host.as_str() {
            "claude-code" => Ok(SessionSource::ClaudeCode),
            "codex-cli" => Ok(SessionSource::Codex),
            "cursor" => Ok(SessionSource::Cursor),
            other => bail!("unsupported Remem session host {other:?}"),
        }
    }

    pub fn is_looper_scheduled(&self) -> bool {
        self.user_message_samples
            .first()
            .is_some_and(|message| super::is_looper_scheduled_skill_first_user_message(message))
    }

    pub fn legacy_document_url(&self) -> String {
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
    host: String,
    source_root: String,
    project: String,
    session_id: String,
    order: String,
    limit: i64,
    count: usize,
    has_more: bool,
    next_cursor: Value,
    content_hash: String,
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
) -> Result<Vec<RememSessionSummary>> {
    let args = strings(&["raw", "sessions", "--sample", "1", "--json"]);
    read_session_summaries(runner, &args, None, 1)
}

#[cfg(test)]
fn load_remem_sessions_with_runner<R: Runner>(runner: &R) -> Result<Vec<RememSession>> {
    load_remem_session_summaries_with_runner(runner)?
        .into_iter()
        .map(|summary| load_one_session(runner, summary))
        .collect()
}

fn read_session_summaries<R: Runner>(
    runner: &R,
    args: &[String],
    expected_project: Option<&str>,
    expected_sample: i64,
) -> Result<Vec<RememSessionSummary>> {
    let mut envelope: SessionsEnvelope = run_json(runner, args, "raw sessions")?;
    ensure!(
        envelope.since_epoch.is_null(),
        "raw sessions unexpectedly applied since_epoch bound: received {:?}",
        envelope.since_epoch
    );
    ensure!(
        envelope.until_epoch.is_null(),
        "raw sessions unexpectedly applied until_epoch bound: received {:?}",
        envelope.until_epoch
    );
    match expected_project {
        Some(expected) => ensure!(
            envelope.project.as_str() == Some(expected),
            "raw sessions project drift: expected {expected:?}, received {:?}",
            envelope.project
        ),
        None => ensure!(
            envelope.project.is_null(),
            "raw sessions unexpectedly applied project filter: received {:?}",
            envelope.project
        ),
    }
    let actual_latest = match &envelope.latest {
        Value::Null => None,
        Value::Number(value) => Some(
            value
                .as_u64()
                .context("raw sessions latest must be a non-negative integer")?,
        ),
        _ => bail!("raw sessions latest has invalid type"),
    };
    ensure!(
        actual_latest.is_none(),
        "raw sessions unexpectedly applied latest bound: received {:?}",
        envelope.latest,
    );
    ensure!(
        envelope.sample == expected_sample,
        "raw sessions sample drifted from {expected_sample}"
    );
    ensure!(
        envelope.count == envelope.sessions.len(),
        "raw sessions count mismatch: declared {}, received {}",
        envelope.count,
        envelope.sessions.len()
    );
    let mut tuples = HashSet::new();
    for summary in &envelope.sessions {
        ensure!(!summary.session_ref.is_empty(), "raw session_ref is empty");
        ensure!(
            summary.session_ref.starts_with("remem://raw-session/v2/"),
            "raw session_ref has unsupported contract version"
        );
        ensure!(
            matches!(
                summary.host.as_str(),
                "claude-code" | "codex-cli" | "cursor"
            ),
            "raw session host is unsupported"
        );
        ensure!(
            matches!(
                summary.session_mode.as_str(),
                "interactive" | "unattended" | "subagent" | "unknown"
            ),
            "raw session mode is unsupported"
        );
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
            summary.user_message_samples.len()
                == usize::try_from(summary.user_message_count.min(expected_sample)).unwrap_or(0),
            "raw sessions returned an unexpected user sample count"
        );
        ensure!(
            summary.content_hash.starts_with("sha256:")
                && summary.content_hash.len() == 71
                && summary.content_hash[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "raw session content_hash is invalid"
        );
        ensure!(
            tuples.insert((
                &summary.host,
                &summary.source_root,
                &summary.project,
                &summary.session_id,
            )),
            "raw sessions returned a duplicate selector tuple"
        );
        let decoded = document::decode_session_ref(&summary.session_ref)?;
        ensure!(
            decoded
                == (
                    summary.host.clone(),
                    summary.source_root.clone(),
                    summary.project.clone(),
                    summary.session_id.clone(),
                ),
            "raw session_ref does not encode its declared selector"
        );
        ensure!(
            summary.content_hash[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
            "raw session content_hash is not hexadecimal"
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
    let session_source = summary.session_source()?;
    let mut result = RememSession {
        session_ref: summary.session_ref.clone(),
        source_root: summary.source_root,
        project: summary.project,
        session_id: summary.session_id,
        first_epoch: summary.first_epoch,
        session: Session {
            source: session_source,
            file_path: PathBuf::new(),
            messages,
            meta: SessionMeta {
                project: None,
                project_identity: None,
                model: None,
                started_at: Some(started_at),
                mode: match summary.session_mode.as_str() {
                    "interactive" => SessionMode::Interactive,
                    "unattended" => SessionMode::Unattended,
                    "subagent" => SessionMode::Subagent,
                    "unknown" => SessionMode::Unknown,
                    _ => unreachable!("session mode validated in summary contract"),
                },
                truncated_tail: false,
            },
        },
    };
    result.session.meta.project = Some(result.project.clone());
    result.session.meta.project_identity = Some(result.project.clone());
    result.session.file_path = PathBuf::from(result.stable_document_url());
    Ok(result)
}

fn validate_page(summary: &RememSessionSummary, envelope: &MessagesEnvelope) -> Result<()> {
    ensure!(
        envelope.source_type == RAW_SOURCE_TYPE,
        "raw messages source_type drift"
    );
    ensure!(envelope.host == summary.host, "raw messages host drift");
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
    ensure!(
        envelope.content_hash.starts_with("sha256:") && envelope.content_hash.len() == 71,
        "raw messages content_hash is invalid"
    );
    ensure!(
        envelope.content_hash[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()),
        "raw messages content_hash is not hexadecimal"
    );
    ensure!(
        envelope.content_hash == summary.content_hash,
        "raw messages snapshot hash drifted from the session summary"
    );
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
        "--host",
        &summary.host,
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
mod tests;
