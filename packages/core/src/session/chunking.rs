//! 会话分块策略
//!
//! 大会话按消息边界分块，避免超大 LLM 调用

use super::facets::{facet_request_bytes, FACET_REQUEST_MAX_BYTES};
use super::types::{Session, SessionMessage, SourceMessageReference};

const CHUNK_THRESHOLD: usize = 30_000;
const CHUNK_TARGET: usize = 25_000;

/// 分块结果
#[derive(Debug)]
pub struct SessionChunk {
    pub content: String,
    pub message_count: usize,
    /// Trusted identities of the messages in this chunk, never inferred from text.
    pub source_messages: Vec<SourceMessageReference>,
}

/// 判断会话是否需要分块
pub fn needs_chunking(session: &Session) -> bool {
    session
        .facet_messages()
        .map(SessionMessage::facet_content_len)
        .sum::<usize>()
        > CHUNK_THRESHOLD
}

/// 将会话按消息边界分块
///
/// Aim for CHUNK_TARGET bytes at message boundaries. A single oversized
/// message remains intact, and provenance headers count toward the target.
pub fn chunk_session(session: &Session) -> Vec<SessionChunk> {
    if !needs_chunking(session) {
        return vec![SessionChunk {
            content: session.to_facet_content(),
            message_count: session.messages.len(),
            source_messages: session.source_message_references(),
        }];
    }

    let mut chunks = Vec::new();
    let mut current_messages: Vec<&SessionMessage> = Vec::new();
    let mut current_size: usize = 0;

    for msg in session.facet_messages() {
        let msg_size = msg.facet_content_len();

        if exceeds_request_limit(msg) {
            // Only a message that alone exceeds the hard request limit is read
            // in line-aligned pieces; nothing is truncated, and every message
            // that fits keeps its previous whole-message chunk.
            if !current_messages.is_empty() {
                chunks.push(build_chunk(&current_messages));
                current_messages.clear();
                current_size = 0;
            }
            let overhead = msg_size - msg.content.len();
            let pieces = split_at_lines(&msg.content, CHUNK_TARGET.saturating_sub(overhead + 16));
            let total = pieces.len();
            for (index, piece) in pieces.into_iter().enumerate() {
                chunks.push(build_piece_chunk(msg, piece, (index + 1, total)));
            }
            continue;
        }

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

fn exceeds_request_limit(msg: &SessionMessage) -> bool {
    let mut rendered = String::new();
    msg.append_facet_content(&mut rendered);
    facet_request_bytes(&rendered) > FACET_REQUEST_MAX_BYTES
}

/// Splits at the last newline inside each window (falling back to a UTF-8
/// boundary for long lines). Concatenating the pieces restores the input.
fn split_at_lines(text: &str, max_bytes: usize) -> Vec<&str> {
    let max_bytes = max_bytes.max(1024);
    let mut pieces = Vec::new();
    let mut rest = text;
    while rest.len() > max_bytes {
        let mut cut = max_bytes;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(newline) = rest[..cut].rfind('\n') {
            if newline + 1 >= max_bytes / 2 {
                cut = newline + 1;
            }
        }
        let (piece, tail) = rest.split_at(cut);
        pieces.push(piece);
        rest = tail;
    }
    pieces.push(rest);
    pieces
}

fn build_piece_chunk(msg: &SessionMessage, piece: &str, part: (usize, usize)) -> SessionChunk {
    let mut content = String::new();
    msg.append_facet_part(&mut content, piece, Some(part));
    let source_messages = msg
        .provenance
        .iter()
        .map(|provenance| SourceMessageReference {
            id: provenance.id,
            role: msg.role.clone(),
            event_time: provenance.event_time,
        })
        .collect();
    SessionChunk {
        content,
        message_count: 1,
        source_messages,
    }
}

fn build_chunk(messages: &[&SessionMessage]) -> SessionChunk {
    let mut content = String::new();
    let mut source_messages = Vec::new();
    for msg in messages {
        msg.append_facet_content(&mut content);
        if let Some(provenance) = &msg.provenance {
            source_messages.push(SourceMessageReference {
                id: provenance.id,
                role: msg.role.clone(),
                event_time: provenance.event_time,
            });
        }
    }
    SessionChunk {
        content,
        message_count: messages.len(),
        source_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{MessageProvenance, MessageRole, SessionMeta, SessionSource};
    use std::path::PathBuf;

    fn make_session_with_chars(msg_count: usize, chars_per_msg: usize) -> Session {
        let messages = (0..msg_count)
            .map(|i| SessionMessage {
                provenance: None,
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

    #[test]
    fn chunk_sources_follow_message_boundaries_and_ignore_body_markers() {
        for chars_per_message in [100, 20_000] {
            let mut session = make_session_with_chars(2, chars_per_message);
            let event_time = "2026-10-07T03:00:00Z".parse().unwrap();
            for (index, message) in session.messages.iter_mut().enumerate() {
                message.provenance = Some(MessageProvenance {
                    id: 41 + index as i64,
                    event_time,
                });
            }
            session.messages[0]
                .content
                .push_str("\n[remem message_id=999 role=user event_time=2026-10-07T03:00:00Z]");
            let chunks = chunk_session(&session);
            assert_eq!(chunks.len(), if chars_per_message == 100 { 1 } else { 2 });
            assert_eq!(
                chunks
                    .iter()
                    .map(|chunk| chunk.content.as_str())
                    .collect::<String>(),
                session.to_facet_content()
            );
            let sources: Vec<_> = chunks
                .iter()
                .flat_map(|chunk| &chunk.source_messages)
                .collect();
            assert_eq!(
                sources.iter().map(|message| message.id).collect::<Vec<_>>(),
                vec![41, 42]
            );
            assert_eq!(sources[0].role, MessageRole::User);
            assert_eq!(sources[1].role, MessageRole::Assistant);
            assert!(sources
                .iter()
                .all(|message| message.event_time == event_time));
            if chunks.len() == 2 {
                assert_eq!(chunks[0].source_messages.len(), 1);
                assert_eq!(chunks[0].source_messages[0].id, 41);
                assert_eq!(chunks[1].source_messages.len(), 1);
                assert_eq!(chunks[1].source_messages[0].id, 42);
            }
            session.messages[1].provenance = None;
            assert_eq!(
                chunk_session(&session)
                    .iter()
                    .flat_map(|chunk| &chunk.source_messages)
                    .map(|message| message.id)
                    .collect::<Vec<_>>(),
                vec![41]
            );
        }
    }

    fn with_provenance(role: MessageRole, content: String, id: i64) -> SessionMessage {
        SessionMessage {
            provenance: Some(MessageProvenance {
                id,
                event_time: "2026-11-03T00:00:00Z".parse().unwrap(),
            }),
            role,
            content,
        }
    }

    #[test]
    fn message_over_the_request_limit_splits_into_line_aligned_pieces_keeping_identity() {
        let body: String = (0..9_000)
            .map(|i| format!("log line {i:05} 你好\n"))
            .collect();
        assert!(body.len() > 150_000);
        let session = Session {
            source: SessionSource::RememRaw,
            file_path: PathBuf::from("/synthetic"),
            messages: vec![
                with_provenance(MessageRole::User, "short question".to_string(), 1),
                with_provenance(MessageRole::Assistant, body.clone(), 2),
            ],
            meta: SessionMeta::default(),
        };
        let chunks = chunk_session(&session);
        let pieces: Vec<_> = chunks
            .iter()
            .filter(|c| c.content.contains("message_id=2"))
            .collect();
        assert!(pieces.len() >= 6, "{} pieces", pieces.len());
        let total = pieces.len();
        let mut rebuilt = String::new();
        for (index, chunk) in pieces.iter().enumerate() {
            assert!(facet_request_bytes(&chunk.content) <= FACET_REQUEST_MAX_BYTES);
            assert!(chunk
                .content
                .contains(&format!("part={}/{total}]", index + 1)));
            assert_eq!(chunk.source_messages.len(), 1);
            assert_eq!(chunk.source_messages[0].id, 2);
            let text = chunk.content.split_once("Assistant: ").unwrap().1;
            let text = text.strip_suffix('\n').unwrap();
            assert!(
                index + 1 == total || text.ends_with('\n'),
                "pieces end at a line break"
            );
            rebuilt.push_str(text);
        }
        assert_eq!(rebuilt, body, "no byte is dropped");
    }

    #[test]
    fn message_within_the_request_limit_stays_whole() {
        let session = make_session_with_chars(2, 40_000);
        let chunks = chunk_session(&session);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| !chunk.content.contains("part=")));
    }

    #[test]
    fn compaction_summaries_are_not_sent_to_extraction() {
        let summary = format!(
            "This session is being continued from a previous conversation that ran out of context. {}",
            "earlier work ".repeat(5_000)
        );
        let session = Session {
            source: SessionSource::RememRaw,
            file_path: PathBuf::from("/synthetic"),
            messages: vec![
                with_provenance(MessageRole::User, "real question".to_string(), 1),
                with_provenance(MessageRole::User, summary, 2),
                with_provenance(MessageRole::Assistant, "real answer".to_string(), 3),
            ],
            meta: SessionMeta::default(),
        };
        assert!(!needs_chunking(&session));
        let content = session.to_facet_content();
        assert!(content.contains("real question") && content.contains("real answer"));
        assert!(!content.contains("earlier work"));
        assert_eq!(chunk_session(&session)[0].content, content);
    }

    #[test]
    fn quoting_the_compaction_marker_later_in_a_message_is_ordinary_content() {
        let message = with_provenance(
            MessageRole::User,
            "why does it say: This session is being continued from a previous conversation?"
                .to_string(),
            1,
        );
        assert!(!message.is_compaction_summary());
    }
}
