//! JSONL 会话解析器
//!
//! 支持 Claude Code 和 Codex 两种格式
//!
//! 解析策略：只容忍 append-only 文件的尾部截断。
//! JSONL 是 append-only 流；当进程被 SIGKILL 或盘满时常见尾行被截断，
//! - 解析成功 → 累积到 messages / meta
//! - 最后一个非空行在 EOF 截断 → 保留合法前缀并标记 `truncated_tail`
//! - 中间损坏或完整写入的非法末行 → 整个文件失败，避免用缺行快照覆盖完整数据
//!
//! 同时从原始 JSONL 提取首个时间戳填充 `SessionMeta.started_at`，
//! 弥补先前的 declaration-execution gap（字段定义但从未写入）。

use super::types::{MessageRole, Session, SessionMessage, SessionMeta, SessionMode, SessionSource};
use chrono::{DateTime, Utc};
use std::path::Path;
use tracing::warn;

/// 单个 session 文件大小上限（200 MiB）。
///
/// 超出后跳过解析，避免 jsonl 异常膨胀触发 OOM 杀掉整个 batch ingest。
/// 对应 HI-6：`std::fs::read_to_string` 默认无上限，本地恶意/损坏/外部共享的
/// jsonl 几 GB 即可耗尽进程内存。
pub const MAX_SESSION_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// 解析 JSONL 文件为 Session
pub fn parse_session_file(path: &Path, source: SessionSource) -> Result<Session, String> {
    if source == SessionSource::RememRaw {
        return Err("RememRaw sessions must be loaded through the remem CLI provider".to_string());
    }
    let metadata = std::fs::metadata(path).map_err(|e| format!("读取文件失败: {}", e))?;
    let size = metadata.len();
    if size > MAX_SESSION_FILE_BYTES {
        let msg = format!(
            "session 文件过大: {} ({} 字节 > 上限 {} 字节)，已跳过",
            path.display(),
            size,
            MAX_SESSION_FILE_BYTES
        );
        tracing::error!(target: "session::parser", path = %path.display(), bytes = size, limit = MAX_SESSION_FILE_BYTES, "session file exceeds size cap, skipping");
        return Err(msg);
    }

    let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;
    parse_session_content(&content, path, source)
}

/// 解析 JSONL 字符串内容为 Session（可测试）
///
/// 中间 JSON 错误返回 `Err`。只有没有换行结尾且 serde 报告 EOF 的最后
/// 一个非空记录会作为 append-in-progress 尾部截断被标记并容忍。
pub fn parse_session_content(
    content: &str,
    path: &Path,
    source: SessionSource,
) -> Result<Session, String> {
    if source == SessionSource::RememRaw {
        return Err("RememRaw sessions cannot be parsed from transcript JSONL".to_string());
    }
    let mut messages = Vec::new();
    let mut meta = SessionMeta::default();
    let last_nonempty_line = content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(idx, _)| idx)
        .last();

    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let is_truncated_tail =
                    Some(idx) == last_nonempty_line && !content.ends_with('\n') && e.is_eof();
                if is_truncated_tail {
                    meta.truncated_tail = true;
                    warn!(
                        path = %path.display(),
                        line = idx + 1,
                        error = %e,
                        "session JSONL has a truncated tail; preserving parsed prefix"
                    );
                    break;
                }
                return Err(format!(
                    "JSONL 解析失败 {}:{}: {}",
                    path.display(),
                    idx + 1,
                    e
                ));
            }
        };

        update_meta_started_at(&value, &mut meta);

        match source {
            SessionSource::ClaudeCode => {
                parse_claude_code_line(&value, &mut messages, &mut meta);
            }
            SessionSource::Codex => {
                parse_codex_line(&value, &mut messages, &mut meta);
            }
            SessionSource::RememRaw => unreachable!("RememRaw is rejected before JSONL parsing"),
        }
    }

    Ok(Session {
        source,
        file_path: path.to_path_buf(),
        messages,
        meta,
    })
}

/// 解析 ISO-8601 字符串为 `DateTime<Utc>`。
fn parse_iso8601_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn project_name_from_cwd(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim();
    if trimmed.is_empty() {
        return None;
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

fn update_codex_session_mode(value: &serde_json::Value, meta: &mut SessionMeta) {
    let observed = if value
        .pointer("/payload/thread_source")
        .and_then(|source| source.as_str())
        == Some("subagent")
    {
        Some(SessionMode::Subagent)
    } else {
        match value
            .pointer("/payload/originator")
            .and_then(|originator| originator.as_str())
        {
            Some("codex-tui" | "Codex Desktop" | "codex_cli_rs") => Some(SessionMode::Interactive),
            Some("codex_exec" | "symphony-orchestrator") => Some(SessionMode::Unattended),
            _ => None,
        }
    };

    if let Some(observed) = observed {
        meta.mode = meta.mode.merge(observed);
    }
}

/// 同时覆盖多种格式：Claude Code 行通常在顶层带 `timestamp`，
/// Codex 新格式也可能把时间放在 `payload.timestamp`。
/// 取首个能解析成 RFC3339 的时间戳作为 `started_at`，与 JSONL 写入顺序对齐。
fn update_meta_started_at(value: &serde_json::Value, meta: &mut SessionMeta) {
    if meta.started_at.is_some() {
        return;
    }
    for pointer in ["/timestamp", "/payload/timestamp"] {
        let Some(ts_str) = value.pointer(pointer).and_then(|v| v.as_str()) else {
            continue;
        };
        if let Some(ts) = parse_iso8601_utc(ts_str) {
            meta.started_at = Some(ts);
            return;
        }
    }
}

/// Claude Code 格式:
/// - `type: "user"` → `message.content` (字符串)
/// - `type: "assistant"` → `message.content` (数组, 提取 type=text 的 text)
/// - `type: "summary"` / `type: "progress"` → 跳过
fn parse_claude_code_line(
    value: &serde_json::Value,
    messages: &mut Vec<SessionMessage>,
    meta: &mut SessionMeta,
) {
    let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "user" => {
            if let Some(text) = extract_claude_code_user_content(value) {
                if !text.is_empty() {
                    messages.push(SessionMessage {
                        role: MessageRole::User,
                        content: text,
                    });
                }
            }
        }
        "assistant" => {
            if let Some(text) = extract_claude_code_assistant_content(value) {
                if !text.is_empty() {
                    messages.push(SessionMessage {
                        role: MessageRole::Assistant,
                        content: text,
                    });
                }
            }
        }
        "system" => {
            // 提取模型信息
            if let Some(model) = value.pointer("/message/model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
            }
        }
        _ => {}
    }
}

fn extract_claude_code_user_content(value: &serde_json::Value) -> Option<String> {
    let content = value.pointer("/message/content")?;

    // 字符串形式
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    // 数组形式
    if let Some(arr) = content.as_array() {
        return Some(extract_text_from_content_array(arr));
    }

    None
}

fn extract_claude_code_assistant_content(value: &serde_json::Value) -> Option<String> {
    let content = value.pointer("/message/content")?;

    if let Some(arr) = content.as_array() {
        return Some(extract_text_from_content_array(arr));
    }

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    None
}

/// 从 content 数组中提取所有 type=text 的 text 字段
fn extract_text_from_content_array(arr: &[serde_json::Value]) -> String {
    extract_text_from_content_array_by_types(arr, &["text"])
}

fn extract_text_from_content_array_by_types(
    arr: &[serde_json::Value],
    allowed_types: &[&str],
) -> String {
    let mut parts = Vec::new();
    for item in arr {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if allowed_types.contains(&item_type) {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text);
            }
        }
    }
    parts.join("\n")
}

/// Codex 格式:
/// - legacy `type: "user_message"` → 顶层 `content`
/// - legacy `type: "response_item"` → `payload.content[].text`
/// - current `type: "response_item"` + `payload.type: "message"`:
///   `payload.role` 区分 user / assistant，`input_text` / `output_text` 存正文
/// - `type: "session_meta"` → 元数据
/// - `type: "turn_context"` → 元数据
/// - `type: "event_msg"` → 跳过
fn parse_codex_line(
    value: &serde_json::Value,
    messages: &mut Vec<SessionMessage>,
    meta: &mut SessionMeta,
) {
    let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "session_meta" => {
            update_codex_session_mode(value, meta);
            if let Some(model) = value
                .pointer("/payload/model")
                .or_else(|| value.get("model"))
                .and_then(|v| v.as_str())
            {
                meta.model = Some(model.to_string());
            }
            if meta.project.is_none() {
                if let Some(cwd) = value.pointer("/payload/cwd").and_then(|v| v.as_str()) {
                    meta.project = project_name_from_cwd(cwd);
                }
            }
        }
        "turn_context" => {
            update_codex_session_mode(value, meta);
            if let Some(model) = value.pointer("/payload/model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
            }
            if meta.project.is_none() {
                if let Some(cwd) = value.pointer("/payload/cwd").and_then(|v| v.as_str()) {
                    meta.project = project_name_from_cwd(cwd);
                }
            }
        }
        "user_message" => {
            if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    messages.push(SessionMessage {
                        role: MessageRole::User,
                        content: text.to_string(),
                    });
                }
            }
        }
        "response_item" => {
            if let Some(message) = parse_codex_response_item_message(value) {
                messages.push(message);
            }
        }
        _ => {}
    }
}

fn parse_codex_response_item_message(value: &serde_json::Value) -> Option<SessionMessage> {
    let arr = value
        .pointer("/payload/content")
        .and_then(|v| v.as_array())?;
    let payload_type = value.pointer("/payload/type").and_then(|v| v.as_str());

    if payload_type == Some("message") {
        let role = value.pointer("/payload/role").and_then(|v| v.as_str())?;
        let (role, allowed_types): (MessageRole, &[&str]) = match role {
            "user" => (MessageRole::User, &["input_text", "text"]),
            "assistant" => (MessageRole::Assistant, &["output_text", "text"]),
            // developer/system instructions are context, not user intent for Mirror metrics.
            _ => return None,
        };
        let text = extract_text_from_content_array_by_types(arr, allowed_types);
        if text.is_empty() {
            return None;
        }
        return Some(SessionMessage {
            role,
            content: text,
        });
    }

    // Legacy Codex response items had no payload.type/role and used content[].type=text.
    if payload_type.is_none() {
        let text = extract_text_from_content_array(arr);
        if !text.is_empty() {
            return Some(SessionMessage {
                role: MessageRole::Assistant,
                content: text,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_remem_raw_as_transcript_input() {
        let path = Path::new("/does/not/need/to/exist.jsonl");
        let file_error = parse_session_file(path, SessionSource::RememRaw).unwrap_err();
        assert!(file_error.contains("remem CLI provider"));

        let content_error = parse_session_content("{}", path, SessionSource::RememRaw).unwrap_err();
        assert!(content_error.contains("cannot be parsed"));
    }

    #[test]
    fn parse_claude_code_session() {
        let jsonl = r#"{"type":"system","message":{"model":"claude-sonnet-4-20250514"}}
{"type":"user","message":{"content":"How do I write tests in Rust?"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Use #[test] attribute."},{"type":"tool_use","name":"bash"}]}}
{"type":"progress","data":"working..."}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/test.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, MessageRole::User);
        assert_eq!(session.messages[0].content, "How do I write tests in Rust?");
        assert_eq!(session.messages[1].role, MessageRole::Assistant);
        assert_eq!(session.messages[1].content, "Use #[test] attribute.");
        assert_eq!(
            session.meta.model.as_deref(),
            Some("claude-sonnet-4-20250514")
        );
    }

    #[test]
    fn parse_codex_session() {
        let jsonl = r#"{"type":"session_meta","model":"o3-mini"}
{"type":"user_message","content":"Fix the bug"}
{"type":"response_item","payload":{"content":[{"type":"text","text":"I found the issue."}]}}
{"type":"event_msg","data":"something"}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/codex.jsonl"),
            SessionSource::Codex,
        )
        .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, MessageRole::User);
        assert_eq!(session.messages[0].content, "Fix the bug");
        assert_eq!(session.messages[1].role, MessageRole::Assistant);
        assert_eq!(session.messages[1].content, "I found the issue.");
        assert_eq!(session.meta.model.as_deref(), Some("o3-mini"));
        assert_eq!(session.meta.mode, SessionMode::Unknown);
    }

    #[test]
    fn parse_codex_classifies_originator_and_thread_source() {
        let cases = [
            (
                r#"{"type":"session_meta","payload":{"originator":"codex-tui","thread_source":"user"}}"#,
                SessionMode::Interactive,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"Codex Desktop"}}"#,
                SessionMode::Interactive,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"codex_cli_rs"}}"#,
                SessionMode::Interactive,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"codex_exec","thread_source":"user"}}"#,
                SessionMode::Unattended,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"symphony-orchestrator"}}"#,
                SessionMode::Unattended,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"codex-tui","thread_source":"subagent"}}"#,
                SessionMode::Subagent,
            ),
            (
                r#"{"type":"session_meta","payload":{"originator":"future-client","thread_source":"user"}}"#,
                SessionMode::Unknown,
            ),
        ];

        for (jsonl, expected) in cases {
            let session = parse_session_content(
                jsonl,
                &PathBuf::from("/tmp/codex-provenance.jsonl"),
                SessionSource::Codex,
            )
            .unwrap();
            assert_eq!(session.meta.mode, expected);
        }
    }

    #[test]
    fn parse_codex_keeps_strongest_provenance_across_records() {
        let jsonl = r#"{"type":"session_meta","payload":{"originator":"codex-tui","thread_source":"subagent"}}
{"type":"turn_context","payload":{"originator":"codex-tui","thread_source":"user"}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/codex-subagent.jsonl"),
            SessionSource::Codex,
        )
        .unwrap();

        assert_eq!(session.meta.mode, SessionMode::Subagent);
    }

    #[test]
    fn parse_codex_current_response_item_schema() {
        let jsonl = r#"{"type":"session_meta","payload":{"timestamp":"2026-05-25T08:00:00Z","model_provider":"openai"}}
{"type":"turn_context","payload":{"cwd":"/Users/lifcc/Desktop/code/AI/tools/refine","model":"gpt-5.3-codex"}}
{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"developer instruction"}]}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix Codex ingest"}]}}
{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"internal"}]}}
{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{}"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Parsed the current schema."}]}}
"#;
        let session = match parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/codex-current.jsonl"),
            SessionSource::Codex,
        ) {
            Ok(session) => session,
            Err(err) => panic!("current Codex schema should parse: {err}"),
        };

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].role, MessageRole::User);
        assert_eq!(session.messages[0].content, "Fix Codex ingest");
        assert_eq!(session.messages[1].role, MessageRole::Assistant);
        assert_eq!(session.messages[1].content, "Parsed the current schema.");
        assert_eq!(session.meta.model.as_deref(), Some("gpt-5.3-codex"));
        assert_eq!(session.meta.project.as_deref(), Some("refine"));
        assert_eq!(
            session.meta.started_at.as_ref().map(|ts| ts.to_rfc3339()),
            Some("2026-05-25T08:00:00+00:00".to_string())
        );
    }

    #[test]
    fn parse_codex_skips_non_transcript_response_items() {
        let jsonl = r#"{"type":"response_item","payload":{"type":"message","role":"system","content":[{"type":"input_text","text":"system"}]}}
{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"developer"}]}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_image","image_url":"file:///tmp/a.png"}]}}
{"type":"response_item","payload":{"type":"function_call_output","output":"tool output"}}
"#;
        let session = match parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/codex-skip.jsonl"),
            SessionSource::Codex,
        ) {
            Ok(session) => session,
            Err(err) => panic!("non-transcript Codex items should be skipped, not fail: {err}"),
        };

        assert_eq!(session.messages.len(), 0);
        assert_eq!(session.user_message_count(), 0);
    }

    #[test]
    fn parse_session_file_rejects_oversize_files() {
        use std::io::Write;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("oversize.jsonl");

        // Write a sparse file just over the configured limit so the test runs cheaply.
        let mut file = std::fs::File::create(&path).expect("create oversize file");
        file.set_len(MAX_SESSION_FILE_BYTES + 1)
            .expect("expand to oversize");
        file.write_all(b"{}").ok();
        drop(file);

        let result = parse_session_file(&path, SessionSource::ClaudeCode);
        assert!(result.is_err(), "oversize file must be rejected");
        let msg = result.err().unwrap();
        assert!(
            msg.contains("过大"),
            "error must mention size cap, got: {msg}"
        );
    }

    #[test]
    fn parse_skips_empty_content() {
        let jsonl = r#"{"type":"user","message":{"content":""}}
{"type":"assistant","message":{"content":[{"type":"text","text":""}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/empty.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert_eq!(session.messages.len(), 0);
    }

    /// 真实场景：JSONL 进程被 SIGKILL 后尾行截断，前面的合法行不应该被丢弃。
    #[test]
    fn parse_recovers_from_truncated_tail() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n\
                     {\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n\
                     {\"type\":\"user\",\"message\":{\"content\":\"oh n";
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/truncated.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "hello");
        assert_eq!(session.messages[1].content, "hi");
        assert!(session.meta.truncated_tail);
    }

    /// 中段单行损坏不能被当成完整 transcript。
    #[test]
    fn parse_recovers_from_midstream_corruption() {
        let jsonl = r#"{"type":"user","message":{"content":"first"}}
{not even json
{"type":"assistant","message":{"content":[{"type":"text","text":"after corruption"}]}}
"#;
        let error = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/midcorrupt.jsonl"),
            SessionSource::ClaudeCode,
        )
        .expect_err("middle corruption must fail the entire session");
        assert!(error.contains(":2:"), "unexpected error: {error}");
    }

    #[test]
    fn invalid_final_record_with_newline_is_not_treated_as_in_progress() {
        let jsonl = "{\"type\":\"user\",\"message\":{\"content\":\"first\"}}\n{bad}\n";
        let error = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/invalid-final.jsonl"),
            SessionSource::ClaudeCode,
        )
        .expect_err("fully written invalid tail must fail");
        assert!(error.contains(":2:"), "unexpected error: {error}");
    }

    #[test]
    fn parse_extracts_started_at_claude_code() {
        let jsonl = r#"{"type":"system","timestamp":"2026-04-21T05:09:08.212Z","message":{"model":"claude-sonnet-4-20250514"}}
{"type":"user","timestamp":"2026-04-21T05:09:09.000Z","message":{"content":"hello"}}
{"type":"assistant","timestamp":"2026-04-21T05:09:10.500Z","message":{"content":[{"type":"text","text":"hi"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/ts.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        let started = session.meta.started_at.expect("started_at should be set");
        assert_eq!(started.to_rfc3339(), "2026-04-21T05:09:08.212+00:00");
    }

    #[test]
    fn parse_extracts_started_at_codex() {
        let jsonl = r#"{"timestamp":"2026-04-10T05:46:47.113Z","type":"session_meta","payload":{},"model":"o3-mini"}
{"timestamp":"2026-04-10T05:46:48.000Z","type":"user_message","content":"go"}
{"timestamp":"2026-04-10T05:46:49.222Z","type":"response_item","payload":{"content":[{"type":"text","text":"ok"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/codex-ts.jsonl"),
            SessionSource::Codex,
        )
        .unwrap();

        let started = session.meta.started_at.expect("started_at should be set");
        assert_eq!(started.to_rfc3339(), "2026-04-10T05:46:47.113+00:00");
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn parse_started_at_remains_none_without_timestamps() {
        let jsonl = r#"{"type":"user","message":{"content":"no ts here"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"nor here"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/no-ts.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert!(session.meta.started_at.is_none());
        assert_eq!(session.messages.len(), 2);
    }

    /// `started_at` 锁定首个合法时间戳；后续行的更晚时间戳不应覆盖它。
    #[test]
    fn parse_started_at_keeps_first_valid_timestamp() {
        let jsonl = r#"{"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"first"}}
{"type":"user","timestamp":"2026-12-31T23:59:59Z","message":{"content":"later"}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/order-ts.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        let started = session.meta.started_at.expect("started_at should be set");
        assert_eq!(started.to_rfc3339(), "2026-01-01T00:00:00+00:00");
    }

    /// 时间戳字符串无法解析时不写入 started_at，但其它字段照常处理，避免静默降级污染元数据。
    #[test]
    fn parse_ignores_unparseable_timestamp_strings() {
        let jsonl = r#"{"type":"user","timestamp":"not-a-real-date","message":{"content":"hello"}}
{"type":"assistant","timestamp":"2026-04-21T05:00:00Z","message":{"content":[{"type":"text","text":"hi"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/bad-ts.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        let started = session
            .meta
            .started_at
            .expect("should fall through to next valid ts");
        assert_eq!(started.to_rfc3339(), "2026-04-21T05:00:00+00:00");
        assert_eq!(session.messages.len(), 2);
    }
}
