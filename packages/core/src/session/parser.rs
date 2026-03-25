//! JSONL 会话解析器
//!
//! 支持 Claude Code 和 Codex 两种格式

use super::types::{MessageRole, Session, SessionMessage, SessionMeta, SessionSource};
use std::path::Path;

/// 解析 JSONL 文件为 Session
pub fn parse_session_file(path: &Path, source: SessionSource) -> Result<Session, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败: {}", e))?;
    parse_session_content(&content, path, source)
}

/// 解析 JSONL 字符串内容为 Session（可测试）
pub fn parse_session_content(
    content: &str,
    path: &Path,
    source: SessionSource,
) -> Result<Session, String> {
    let mut messages = Vec::new();
    let mut meta = SessionMeta::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("JSON 解析失败: {}", e))?;

        match source {
            SessionSource::ClaudeCode => {
                parse_claude_code_line(&value, &mut messages, &mut meta);
            }
            SessionSource::Codex => {
                parse_codex_line(&value, &mut messages, &mut meta);
            }
        }
    }

    Ok(Session {
        source,
        file_path: path.to_path_buf(),
        messages,
        meta,
    })
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
}
