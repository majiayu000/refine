//! 知识模块的值对象
//!
//! 不可变类型，按值比较

use crate::error::DomainError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ════════════════════════════════════════════════════════════
// ItemId - 知识片段唯一标识
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(String);

impl ItemId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ItemId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ════════════════════════════════════════════════════════════
// Tag - 标签
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tag(String);

impl Tag {
    pub fn new(value: &str) -> Result<Self, DomainError> {
        let value = value.trim().to_lowercase();

        if value.is_empty() {
            return Err(DomainError::Validation("标签不能为空".into()));
        }
        if value.len() > 50 {
            return Err(DomainError::Validation("标签过长".into()));
        }

        Ok(Self(value))
    }

    /// 尝试创建，失败返回 None
    pub fn try_new(value: &str) -> Option<Self> {
        Self::new(value).ok()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ════════════════════════════════════════════════════════════
// Source - 来源信息
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub platform: String,
    pub conversation_id: Option<String>,
    pub url: Option<String>,
}

impl Source {
    pub fn new(platform: &str) -> Self {
        Self {
            platform: platform.to_string(),
            conversation_id: None,
            url: None,
        }
    }

    pub fn with_conversation_id(mut self, id: &str) -> Self {
        self.conversation_id = Some(id.to_string());
        self
    }

    pub fn with_url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        self
    }
}

// ════════════════════════════════════════════════════════════
// ItemType - 知识片段类型
// ════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Knowledge,
    Skill,
    Snippet,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Skill => "skill",
            Self::Snippet => "snippet",
        }
    }
}
