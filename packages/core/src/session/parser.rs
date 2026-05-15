//! JSONL 会话解析器
//!
//! 支持 Claude Code 和 Codex 两种格式
//!
//! 解析策略：line-level error recovery。
//! JSONL 是 append-only 流；当进程被 SIGKILL 或盘满时常见尾行被截断，
//! 中途偶发损坏行不应该让整个会话被丢弃。每条 line 独立解析：
//! - 解析成功 → 累积到 messages / meta
//! - 解析失败 → 记 warn 日志（带 line index 与截断 snippet），继续下一行
//!
//! 同时从原始 JSONL 提取首个时间戳填充 `SessionMeta.started_at`，
//! 弥补先前的 declaration-execution gap（字段定义但从未写入）。

use super::types::{MessageRole, Session, SessionMessage, SessionMeta, SessionSource};
use chrono::{DateTime, Utc};
use std::path::Path;
use tracing::warn;

/// 单行 snippet 在日志里的最大长度，避免泄漏整条原始内容到日志后端。
const SNIPPET_MAX: usize = 120;

/// 单个 session 文件大小上限（200 MiB）。
///
/// 超出后跳过解析，避免 jsonl 异常膨胀触发 OOM 杀掉整个 batch ingest。
/// 对应 HI-6：`std::fs::read_to_string` 默认无上限，本地恶意/损坏/外部共享的
/// jsonl 几 GB 即可耗尽进程内存。
pub const MAX_SESSION_FILE_BYTES: u64 = 200 * 1024 * 1024;

/// 解析 JSONL 文件为 Session
pub fn parse_session_file(path: &Path, source: SessionSource) -> Result<Session, String> {
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
/// 返回 `Err` 仅在调用方传入完全无法处理的输入时；单行 JSON 错误不会
/// 中断整个文件解析，遵循 JSONL append-only 流的容错惯例。
pub fn parse_session_content(
    content: &str,
    path: &Path,
    source: SessionSource,
) -> Result<Session, String> {
    let mut messages = Vec::new();
    let mut meta = SessionMeta::default();
    let mut malformed = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                malformed += 1;
                warn!(
                    path = %path.display(),
                    line = idx + 1,
                    error = %e,
                    snippet = %truncate_snippet(line),
                    "skipping malformed JSONL line"
                );
                continue;
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
        }
    }

    if malformed > 0 {
        warn!(
            path = %path.display(),
            malformed,
            kept = messages.len(),
            "JSONL parse completed with skipped lines"
        );
    }

    Ok(Session {
        source,
        file_path: path.to_path_buf(),
        messages,
        meta,
    })
}

/// 截断单行内容用于日志输出。
fn truncate_snippet(line: &str) -> String {
    if line.len() <= SNIPPET_MAX {
        line.to_string()
    } else {
        // char_indices 避免在 UTF-8 字符中间切断。
        let cut = line
            .char_indices()
            .take_while(|(i, _)| *i < SNIPPET_MAX)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}…", &line[..cut])
    }
}

/// 解析 ISO-8601 字符串为 `DateTime<Utc>`。
fn parse_iso8601_utc(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// 同时覆盖两种格式：Claude Code 与 Codex 行均在顶层带 `timestamp` 字段。
/// 取首个能解析成 RFC3339 的时间戳作为 `started_at`，与 JSONL 写入顺序对齐。
fn update_meta_started_at(value: &serde_json::Value, meta: &mut SessionMeta) {
    if meta.started_at.is_some() {
        return;
    }
    let Some(ts_str) = value.get("timestamp").and_then(|v| v.as_str()) else {
        return;
    };
    if let Some(ts) = parse_iso8601_utc(ts_str) {
        meta.started_at = Some(ts);
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
    let mut parts = Vec::new();
    for item in arr {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type == "text" {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                parts.push(text);
            }
        }
    }
    parts.join("\n")
}

/// Codex 格式:
/// - `type: "response_item"` → `payload.content[].text`
/// - `type: "session_meta"` → 元数据
/// - `type: "turn_context"` / `type: "event_msg"` → 跳过
fn parse_codex_line(
    value: &serde_json::Value,
    messages: &mut Vec<SessionMessage>,
    meta: &mut SessionMeta,
) {
    let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "session_meta" => {
            if let Some(model) = value.get("model").and_then(|v| v.as_str()) {
                meta.model = Some(model.to_string());
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
            if let Some(arr) = value.pointer("/payload/content").and_then(|v| v.as_array()) {
                let text = extract_text_from_content_array(arr);
                if !text.is_empty() {
                    messages.push(SessionMessage {
                        role: MessageRole::Assistant,
                        content: text,
                    });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
    }

    /// 中段单行损坏：仅丢弃损坏行，前后行正常累积。
    #[test]
    fn parse_recovers_from_midstream_corruption() {
        let jsonl = r#"{"type":"user","message":{"content":"first"}}
{not even json
{"type":"assistant","message":{"content":[{"type":"text","text":"after corruption"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/midcorrupt.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0].content, "first");
        assert_eq!(session.messages[1].content, "after corruption");
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

    #[test]
    fn truncate_snippet_handles_utf8_boundaries() {
        // 多字节字符在 SNIPPET_MAX 边界附近时不应导致越界 panic。
        let s: String = "中".repeat(80); // 240 bytes, well past SNIPPET_MAX
        let out = truncate_snippet(&s);
        assert!(out.ends_with('…'));
        // 必须仍是合法 UTF-8（如果切坏 String 构造会 panic）
        assert!(!out.is_empty());
    }

    #[test]
    fn truncate_snippet_passthrough_for_short_lines() {
        let out = truncate_snippet("short");
        assert_eq!(out, "short");
    }
}
