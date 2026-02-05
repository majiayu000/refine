//! 对话解析
//!
//! 将原始对话文本解析为结构化格式

use crate::error::DomainError;

/// 对话角色
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Human,
    Assistant,
}

/// 单条消息
#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

/// 解析后的对话
#[derive(Debug, Clone)]
pub struct Conversation {
    pub messages: Vec<Message>,
    pub raw: String,
}

impl Conversation {
    /// 解析对话文本
    pub fn parse(text: &str) -> Result<Self, DomainError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(DomainError::InvalidConversation);
        }

        let messages = Self::detect_and_parse(text)?;

        if messages.is_empty() {
            return Err(DomainError::InvalidConversation);
        }

        Ok(Self {
            messages,
            raw: text.to_string(),
        })
    }

    /// 检测格式并解析
    fn detect_and_parse(text: &str) -> Result<Vec<Message>, DomainError> {
        // 尝试不同格式
        if let Some(msgs) = Self::parse_human_assistant(text) {
            return Ok(msgs);
        }
        if let Some(msgs) = Self::parse_qa_format(text) {
            return Ok(msgs);
        }
        if let Some(msgs) = Self::parse_user_ai(text) {
            return Ok(msgs);
        }

        // 无法识别格式，整体作为一条消息
        Ok(vec![Message {
            role: Role::Human,
            content: text.to_string(),
        }])
    }

    /// Human: / Assistant: 格式
    fn parse_human_assistant(text: &str) -> Option<Vec<Message>> {
        if !text.contains("Human:") && !text.contains("Assistant:") {
            return None;
        }

        let mut messages = Vec::new();
        let mut current_role: Option<Role> = None;
        let mut current_content = String::new();

        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("Human:") {
                if let Some(role) = current_role.take() {
                    messages.push(Message {
                        role,
                        content: current_content.trim().to_string(),
                    });
                }
                current_role = Some(Role::Human);
                current_content = trimmed.strip_prefix("Human:").unwrap().to_string();
            } else if trimmed.starts_with("Assistant:") {
                if let Some(role) = current_role.take() {
                    messages.push(Message {
                        role,
                        content: current_content.trim().to_string(),
                    });
                }
                current_role = Some(Role::Assistant);
                current_content = trimmed.strip_prefix("Assistant:").unwrap().to_string();
            } else if current_role.is_some() {
                current_content.push('\n');
                current_content.push_str(line);
            }
        }

        if let Some(role) = current_role {
            messages.push(Message {
                role,
                content: current_content.trim().to_string(),
            });
        }

        if messages.is_empty() {
            None
        } else {
            Some(messages)
        }
    }

    /// Q: / A: 格式
    fn parse_qa_format(text: &str) -> Option<Vec<Message>> {
        if !text.contains("Q:") && !text.contains("A:") {
            return None;
        }

        let mut messages = Vec::new();
        let mut current_role: Option<Role> = None;
        let mut current_content = String::new();

        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("Q:") {
                if let Some(role) = current_role.take() {
                    messages.push(Message {
                        role,
                        content: current_content.trim().to_string(),
                    });
                }
                current_role = Some(Role::Human);
                current_content = trimmed.strip_prefix("Q:").unwrap().to_string();
            } else if trimmed.starts_with("A:") {
                if let Some(role) = current_role.take() {
                    messages.push(Message {
                        role,
                        content: current_content.trim().to_string(),
                    });
                }
                current_role = Some(Role::Assistant);
                current_content = trimmed.strip_prefix("A:").unwrap().to_string();
            } else if current_role.is_some() {
                current_content.push('\n');
                current_content.push_str(line);
            }
        }

        if let Some(role) = current_role {
            messages.push(Message {
                role,
                content: current_content.trim().to_string(),
            });
        }

        if messages.is_empty() {
            None
        } else {
            Some(messages)
        }
    }

    /// User: / AI: 格式
    fn parse_user_ai(text: &str) -> Option<Vec<Message>> {
        if !text.contains("User:") && !text.contains("AI:") {
            return None;
        }

        let text = text.replace("User:", "Human:").replace("AI:", "Assistant:");
        Self::parse_human_assistant(&text)
    }

    /// 获取用户问题（第一条 Human 消息）
    pub fn get_question(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|m| m.role == Role::Human)
            .map(|m| m.content.as_str())
    }

    /// 获取 AI 回答（最后一条 Assistant 消息）
    pub fn get_answer(&self) -> Option<&str> {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.as_str())
    }
}
