//! 会话质量过滤
//!
//! 过滤低信号会话，保留值得提取的内容

use super::types::{MessageRole, Session};

const LOOPER_SCHEDULED_SKILL_MARKER: &str = "You are executing Looper scheduled skill";

/// 过滤阈值配置
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// 最少用户消息数
    pub min_user_messages: usize,
    /// 最少总字符数
    pub min_char_count: usize,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            min_user_messages: 1,
            min_char_count: 500,
        }
    }
}

/// 判断会话是否通过质量阈值
pub fn passes_filter(session: &Session, config: &FilterConfig) -> bool {
    session.user_message_count() >= config.min_user_messages
        && session.char_count() >= config.min_char_count
        && !is_looper_scheduled_skill_session(session)
}

/// Looper scheduled jobs declare themselves at the start of the first user
/// message. Mentions elsewhere are ordinary discussion and remain eligible.
pub fn is_looper_scheduled_skill_session(session: &Session) -> bool {
    session
        .messages
        .iter()
        .find(|message| message.role == MessageRole::User)
        .is_some_and(|message| message.content.starts_with(LOOPER_SCHEDULED_SKILL_MARKER))
}

/// 批量过滤会话
pub fn filter_sessions(sessions: Vec<Session>, config: &FilterConfig) -> Vec<Session> {
    sessions
        .into_iter()
        .filter(|s| passes_filter(s, config))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{MessageRole, SessionMessage, SessionMeta, SessionSource};
    use std::path::PathBuf;

    fn make_session(user_msgs: usize, char_per_msg: usize) -> Session {
        let mut messages = Vec::new();
        for _ in 0..user_msgs {
            messages.push(SessionMessage {
                role: MessageRole::User,
                content: "x".repeat(char_per_msg),
            });
            messages.push(SessionMessage {
                role: MessageRole::Assistant,
                content: "y".repeat(char_per_msg),
            });
        }
        Session {
            source: SessionSource::ClaudeCode,
            file_path: PathBuf::from("/tmp/test.jsonl"),
            messages,
            meta: SessionMeta::default(),
        }
    }

    #[test]
    fn filter_rejects_short_sessions() {
        let config = FilterConfig::default();
        let short = make_session(1, 50);
        assert!(!passes_filter(&short, &config));
    }

    #[test]
    fn filter_accepts_substantial_sessions() {
        let config = FilterConfig::default();
        let good = make_session(3, 200);
        assert!(passes_filter(&good, &config));
    }

    #[test]
    fn filter_sessions_removes_low_quality() {
        let config = FilterConfig::default();
        let sessions = vec![
            make_session(1, 10),
            make_session(3, 200),
            make_session(1, 50),
        ];
        let filtered = filter_sessions(sessions, &config);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_rejects_explicit_looper_scheduled_skill_marker() {
        let mut session = make_session(3, 200);
        session.messages[0].content =
            "You are executing Looper scheduled skill \"daily\".\nFollow the spec.".to_string();

        assert!(is_looper_scheduled_skill_session(&session));
        assert!(!passes_filter(&session, &FilterConfig::default()));
    }

    #[test]
    fn filter_keeps_normal_discussion_that_only_mentions_looper_marker() {
        let mut session = make_session(3, 200);
        session.messages[0].content = format!(
            "Please explain why a transcript says {LOOPER_SCHEDULED_SKILL_MARKER:?}. {}",
            "context ".repeat(80)
        );

        assert!(!is_looper_scheduled_skill_session(&session));
        assert!(passes_filter(&session, &FilterConfig::default()));
    }

    #[test]
    fn only_the_first_user_message_can_mark_a_looper_session() {
        let mut session = make_session(3, 200);
        session.messages[2].content =
            "You are executing Looper scheduled skill \"quoted\".".to_string();

        assert!(!is_looper_scheduled_skill_session(&session));
        assert!(passes_filter(&session, &FilterConfig::default()));
    }
}
