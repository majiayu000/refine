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

use std::path::Path;
use tracing::warn;

/// 单个 session 文件大小上限（200 MiB）。
///
/// 超出后跳过解析，避免 jsonl 异常膨胀触发 OOM 杀掉整个 batch ingest。
/// 对应 HI-6：`std::fs::read_to_string` 默认无上限，本地恶意/损坏/外部共享的
/// jsonl 几 GB 即可耗尽进程内存。
pub const MAX_SESSION_FILE_BYTES: u64 = 200 * 1024 * 1024;

fn reject_remem_backed_source(source: &SessionSource) -> Result<(), String> {
    match source {
        SessionSource::Cursor => {
            Err("Cursor sessions must be loaded through the remem CLI provider".to_string())
        }
        SessionSource::RememRaw => {
            Err("RememRaw sessions must be loaded through the remem CLI provider".to_string())
        }
        SessionSource::ClaudeCode | SessionSource::Codex => Ok(()),
    }
}

/// 解析 JSONL 文件为 Session
pub fn parse_session_file(path: &Path, source: SessionSource) -> Result<Session, String> {
    reject_remem_backed_source(&source)?;
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
    reject_remem_backed_source(&source)?;
    let mut messages = Vec::new();
    let mut meta = SessionMeta::default();
    let last_nonempty_line = content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(idx, _)| idx)
        .last();

    let agent = match source {
        SessionSource::ClaudeCode => agent_sessions::Agent::ClaudeCode,
        SessionSource::Codex => agent_sessions::Agent::Codex,
        SessionSource::Cursor | SessionSource::RememRaw => unreachable!("checked above"),
    };
    let records = agent_sessions::read_raw_from(
        std::io::Cursor::new(content.as_bytes()),
        &agent_sessions::RawReadOptions {
            max_read_bytes: None,
            max_line_bytes: None,
            ..Default::default()
        },
    )
    .map_err(|error| format!("读取文件失败: {error}"))?;
    for record in records {
        let record = record.map_err(|error| format!("读取文件失败: {error}"))?;
        let line = std::str::from_utf8(&record.bytes)
            .map_err(|error| format!("读取文件失败: {error}"))?
            .trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                let is_truncated_tail = last_nonempty_line == Some(record.line_no as usize - 1)
                    && !content.ends_with('\n')
                    && error.is_eof();
                if is_truncated_tail {
                    meta.truncated_tail = true;
                    warn!(path = %path.display(), line = record.line_no, %error,
                        "session JSONL has a truncated tail; preserving parsed prefix");
                    break;
                }
                return Err(format!(
                    "JSONL 解析失败 {}:{}: {}",
                    path.display(),
                    record.line_no,
                    error
                ));
            }
        };
        let projected = agent_sessions::project_transcript(agent, &value);
        if meta.started_at.is_none() {
            meta.started_at = projected.at;
        }
        if let Some(cwd) = projected.meta.cwd {
            update_project_from_cwd(&cwd, &mut meta);
        }
        if let Some(model) = projected.meta.model {
            meta.model = Some(model);
        }
        if let Some(origin) = projected.meta.origin {
            let mode = match origin {
                agent_sessions::Origin::Subagent => SessionMode::Subagent,
                agent_sessions::Origin::Exec => SessionMode::Unattended,
                agent_sessions::Origin::Interactive | agent_sessions::Origin::Ide => {
                    SessionMode::Interactive
                }
                _ => SessionMode::Unknown,
            };
            meta.mode = meta.mode.merge(mode);
        }
        if let Some(message) = projected
            .message
            .filter(|m| !m.is_meta && !m.text.is_empty())
        {
            let role = match message.role {
                agent_sessions::Role::User => MessageRole::User,
                agent_sessions::Role::Assistant => MessageRole::Assistant,
                _ => continue,
            };
            messages.push(SessionMessage {
                role,
                content: message.text,
            });
        }
    }

    Ok(Session {
        source,
        file_path: path.to_path_buf(),
        messages,
        meta,
    })
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

fn update_project_from_cwd(cwd: &str, meta: &mut SessionMeta) {
    let identity = cwd.trim();
    if identity.is_empty() {
        return;
    }
    if meta.project_identity.is_none() {
        meta.project_identity = Some(identity.to_string());
    }
    if meta.project.is_none() {
        meta.project = project_name_from_cwd(identity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_remem_backed_sources_before_file_or_content_parsing() {
        let path = Path::new("/does/not/need/to/exist.jsonl");
        for (source, name) in [
            (SessionSource::Cursor, "Cursor"),
            (SessionSource::RememRaw, "RememRaw"),
        ] {
            let file_error = parse_session_file(path, source.clone()).unwrap_err();
            assert_eq!(
                file_error,
                format!("{name} sessions must be loaded through the remem CLI provider")
            );

            let content_error = parse_session_content("not JSON", path, source).unwrap_err();
            assert_eq!(
                content_error,
                format!("{name} sessions must be loaded through the remem CLI provider")
            );
        }
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
            session.meta.project_identity.as_deref(),
            Some("/Users/lifcc/Desktop/code/AI/tools/refine")
        );
        assert_eq!(
            session.meta.started_at.as_ref().map(|ts| ts.to_rfc3339()),
            Some("2026-05-25T08:00:00+00:00".to_string())
        );
    }

    #[test]
    fn parse_claude_preserves_raw_cwd_without_changing_display_project() {
        let jsonl = r#"{"type":"user","cwd":"/r/Foo/bar","message":{"content":"hello"}}
{"type":"assistant","cwd":"/r/Foo/bar","message":{"content":[{"type":"text","text":"hi"}]}}
"#;
        let session = parse_session_content(
            jsonl,
            &PathBuf::from("/tmp/claude-cwd.jsonl"),
            SessionSource::ClaudeCode,
        )
        .unwrap();

        assert_eq!(session.meta.project.as_deref(), Some("bar"));
        assert_eq!(session.meta.project_identity.as_deref(), Some("/r/Foo/bar"));
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
    #[test]
    fn shared_provenance_prioritizes_source_and_subagent() {
        for (payload, expected) in [
            (
                r#"{"source":"exec","originator":"Codex Desktop"}"#,
                SessionMode::Unattended,
            ),
            (
                r#"{"source":"vscode","originator":"unknown"}"#,
                SessionMode::Interactive,
            ),
            (
                r#"{"source":{"subagent":{"parent_thread_id":"parent"}},"originator":"codex-tui"}"#,
                SessionMode::Subagent,
            ),
        ] {
            let content = format!(r#"{{"type":"session_meta","payload":{payload}}}"#);
            let session =
                parse_session_content(&content, Path::new("test.jsonl"), SessionSource::Codex)
                    .unwrap();
            assert_eq!(session.meta.mode, expected);
        }
    }

    #[test]
    fn meta_messages_do_not_count_as_user_intent() {
        let content = r#"{"type":"user","isMeta":true,"message":{"content":"internal"}}
{"type":"user","message":{"content":"real request"}}
"#;
        let session =
            parse_session_content(content, Path::new("test.jsonl"), SessionSource::ClaudeCode)
                .unwrap();
        assert_eq!(session.user_message_count(), 1);
        assert_eq!(session.messages[0].content, "real request");
    }
    #[test]
    fn preserves_unicode_whitespace_and_blank_final_tail_policy() {
        let content = "\u{2003}{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\u{2003}\n{\"type\":\n ";
        let session =
            parse_session_content(content, Path::new("test.jsonl"), SessionSource::ClaudeCode)
                .unwrap();
        assert_eq!(session.messages[0].content, "hello");
        assert!(session.meta.truncated_tail);
    }
}
