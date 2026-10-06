//! 会话类型定义
//!
//! 统一表示 remem raw archive 和旧 Claude Code/Codex 文件扫描的会话数据

use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// 会话来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSource {
    ClaudeCode,
    Codex,
    Cursor,
    RememRaw,
}

impl SessionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code-session",
            Self::Codex => "codex-session",
            Self::Cursor => "cursor-session",
            Self::RememRaw => "remem-raw-session",
        }
    }
}

/// 消息角色
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

impl MessageRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageProvenance {
    pub id: i64,
    pub event_time: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SourceMessageReference {
    pub id: i64,
    pub role: MessageRole,
    pub event_time: DateTime<Utc>,
}

/// Provenance reported by the Codex transcript metadata.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionMode {
    Interactive,
    Unattended,
    Subagent,
    #[default]
    Unknown,
}

impl SessionMode {
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Interactive => "session_mode_interactive",
            Self::Unattended => "session_mode_unattended",
            Self::Subagent => "session_mode_subagent",
            Self::Unknown => "session_mode_unknown",
        }
    }

    pub(super) fn merge(self, observed: Self) -> Self {
        fn precedence(mode: SessionMode) -> u8 {
            match mode {
                SessionMode::Unknown => 0,
                SessionMode::Interactive => 1,
                SessionMode::Unattended => 2,
                SessionMode::Subagent => 3,
            }
        }

        if precedence(observed) > precedence(self) {
            observed
        } else {
            self
        }
    }
}

/// 会话消息
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub role: MessageRole,
    pub content: String,
    /// Present only when a source provides a real message identity and time.
    pub provenance: Option<MessageProvenance>,
}

impl SessionMessage {
    pub(super) fn facet_content_len(&self) -> usize {
        let label_len = match self.role {
            MessageRole::User => 6,
            MessageRole::Assistant => 11,
            MessageRole::System => 8,
        };
        self.content.len()
            + label_len
            + 1
            + self.provenance.as_ref().map_or(0, |provenance| {
                format!(
                    "[remem message_id={} role={} event_time={}]\n",
                    provenance.id,
                    self.role.as_str(),
                    provenance.event_time.to_rfc3339()
                )
                .len()
            })
    }

    pub(super) fn append_facet_content(&self, output: &mut String) {
        if let Some(provenance) = &self.provenance {
            output.push_str(&format!(
                "[remem message_id={} role={} event_time={}]\n",
                provenance.id,
                self.role.as_str(),
                provenance.event_time.to_rfc3339()
            ));
        }
        let label = match self.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::System => "System",
        };
        output.push_str(label);
        output.push_str(": ");
        output.push_str(&self.content);
        output.push('\n');
    }
}

/// 会话元数据
#[derive(Debug, Clone, Default)]
pub struct SessionMeta {
    /// Backward-compatible display name used by existing ingest tags and UI.
    pub project: Option<String>,
    /// Exact project identity before basename/tag normalization. Codex and
    /// Claude cwd paths retain case and punctuation here for collision-aware
    /// snapshot resolution.
    pub project_identity: Option<String>,
    pub model: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub mode: SessionMode,
    /// The final JSONL record ended at EOF before it became valid JSON. The
    /// parsed prefix is useful for inspection, but must not replace a complete
    /// persisted snapshot or advance an incremental ingest cursor.
    pub truncated_tail: bool,
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
    /// Extraction view; legacy canonical text and Remem snapshot hashes retain
    /// their previous format. Provenance is data, never inferred from text.
    pub fn to_facet_content(&self) -> String {
        let mut out = String::new();
        for message in &self.messages {
            message.append_facet_content(&mut out);
        }
        out
    }

    pub fn source_message_references(&self) -> Vec<SourceMessageReference> {
        self.messages
            .iter()
            .filter_map(|message| {
                message
                    .provenance
                    .as_ref()
                    .map(|provenance| SourceMessageReference {
                        id: provenance.id,
                        role: message.role.clone(),
                        event_time: provenance.event_time,
                    })
            })
            .collect()
    }

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
                    provenance: None,
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

    #[test]
    fn remem_raw_has_distinct_document_source() {
        assert_eq!(SessionSource::RememRaw.as_str(), "remem-raw-session");
    }
}
