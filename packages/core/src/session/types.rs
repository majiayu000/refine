//! 会话类型定义
//!
//! 统一表示 Claude Code 和 Codex 的会话数据

use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// 会话来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSource {
    ClaudeCode,
    Codex,
}

impl SessionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code-session",
            Self::Codex => "codex-session",
        }
    }
}

/// 消息角色
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// 会话消息
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
}

/// 会话元数据
#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    pub project: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
}

/// 统一会话结构
#[derive(Debug, Clone)]
pub struct Session {
    pub source: SessionSource,
    pub file_path: PathBuf,
    pub messages: Vec<SessionMessage>,
    pub meta: SessionMeta,
}

impl Session {
    /// 将消息拼接为纯文本内容（用于存储为 Document.raw_content）
    pub fn to_document_content(&self) -> String {
        let mut out = String::new();
        for msg in &self.messages {
            let role_label = match msg.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::System => "System",
            };
            out.push_str(role_label);
            out.push_str(": ");
            out.push_str(&msg.content);
            out.push('\n');
        }
        out
    }

    /// 质量阈值检查：是否值得提取
    pub fn is_substantial(&self) -> bool {
        let user_count = self
            .messages
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .count();
        let char_count: usize = self.messages.iter().map(|m| m.content.len()).sum();
        user_count >= 2 && char_count >= 500
    }

    /// 总字符数
    pub fn char_count(&self) -> usize {
        self.messages.iter().map(|m| m.content.len()).sum()
    }

    /// 用户消息数
    pub fn user_message_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m.role == MessageRole::User)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_session(messages: Vec<(MessageRole, &str)>) -> Session {
        Session {
            source: SessionSource::ClaudeCode,
            file_path: PathBuf::from("/tmp/test.jsonl"),
            messages: messages
                .into_iter()
                .map(|(role, content)| SessionMessage {
                    role,
                    content: content.to_string(),
                })
                .collect(),
            meta: SessionMeta::default(),
        }
    }

    #[test]
    fn to_document_content_joins_messages() {
        let session = make_session(vec![
            (MessageRole::User, "hello"),
            (MessageRole::Assistant, "hi"),
        ]);
        let content = session.to_document_content();
        assert!(content.contains("User: hello"));
        assert!(content.contains("Assistant: hi"));
    }

    #[test]
    fn is_substantial_requires_min_messages_and_chars() {
        let short = make_session(vec![(MessageRole::User, "hi")]);
        assert!(!short.is_substantial());

        let long_text = "x".repeat(500);
        let substantial = make_session(vec![
            (MessageRole::User, &long_text),
            (MessageRole::User, "more"),
        ]);
        assert!(substantial.is_substantial());
    }
}
