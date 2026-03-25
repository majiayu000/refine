//! 会话分块策略
//!
//! 大会话按消息边界分块，避免超大 LLM 调用

use super::types::{MessageRole, Session, SessionMessage};

const CHUNK_THRESHOLD: usize = 30_000;
const CHUNK_TARGET: usize = 25_000;

/// 分块结果
#[derive(Debug)]
pub struct SessionChunk {
    pub content: String,
    pub message_count: usize,
}

/// 判断会话是否需要分块
pub fn needs_chunking(session: &Session) -> bool {
    session.char_count() > CHUNK_THRESHOLD
}

/// 将会话按消息边界分块
///
/// 每块不超过 CHUNK_TARGET 字符，不在消息中间切断
pub fn chunk_session(session: &Session) -> Vec<SessionChunk> {
    if !needs_chunking(session) {
        return vec![SessionChunk {
            content: session.to_document_content(),
            message_count: session.messages.len(),
        }];
    }

    let mut chunks = Vec::new();
    let mut current_messages: Vec<&SessionMessage> = Vec::new();
    let mut current_size: usize = 0;

    for msg in &session.messages {
        let msg_size = msg.content.len() + role_label_len(&msg.role);

        if current_size + msg_size > CHUNK_TARGET && !current_messages.is_empty() {
            chunks.push(build_chunk(&current_messages));
            current_messages.clear();
            current_size = 0;
        }

        current_messages.push(msg);
        current_size += msg_size;
    }

    if !current_messages.is_empty() {
        chunks.push(build_chunk(&current_messages));
    }

    chunks
}

fn build_chunk(messages: &[&SessionMessage]) -> SessionChunk {
    let mut content = String::new();
    for msg in messages {
        let label = match msg.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };
        content.push_str(label);
        content.push_str(": ");
        content.push_str(&msg.content);
        content.push('\n');
    }
    SessionChunk {
        content,
        message_count: messages.len(),
    }
}

fn role_label_len(role: &MessageRole) -> usize {
    match role {
        MessageRole::User => 6,       // "User: "
        MessageRole::Assistant => 12, // "Assistant: "
        MessageRole::System => 9,     // "System: "
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionMeta, SessionSource};
    use std::path::PathBuf;

    fn make_session_with_chars(msg_count: usize, chars_per_msg: usize) -> Session {
        let messages = (0..msg_count)
            .map(|i| SessionMessage {
                role: if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                content: "x".repeat(chars_per_msg),
            })
            .collect();

        Session {
            source: SessionSource::ClaudeCode,
            file_path: PathBuf::from("/tmp/test.jsonl"),
            messages,
            meta: SessionMeta::default(),
        }
    }

    #[test]
    fn small_session_produces_single_chunk() {
        let session = make_session_with_chars(4, 100);
        let chunks = chunk_session(&session);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].message_count, 4);
    }

    #[test]
    fn large_session_splits_at_message_boundaries() {
        // 10 messages * 5000 chars = 50000 > 30000 threshold
        let session = make_session_with_chars(10, 5000);
        assert!(needs_chunking(&session));

        let chunks = chunk_session(&session);
        assert!(chunks.len() > 1);

        // 所有消息都应被包含
        let total_msgs: usize = chunks.iter().map(|c| c.message_count).sum();
        assert_eq!(total_msgs, 10);

        // 每块内容不应为空
        for chunk in &chunks {
            assert!(!chunk.content.is_empty());
        }
    }
}
