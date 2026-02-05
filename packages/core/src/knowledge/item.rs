//! Item 聚合根
//!
//! 知识片段的核心实体，控制内部一致性

use crate::error::DomainError;
use crate::knowledge::types::{ItemId, ItemType, Source, Tag};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 知识片段 - 聚合根
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    id: ItemId,
    item_type: ItemType,
    title: String,
    summary: String,
    content: String,
    tags: Vec<Tag>,
    source: Option<Source>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Item {
    // ────────────────────────────────────────────────────────
    // 构造器
    // ────────────────────────────────────────────────────────

    pub fn new_knowledge(title: &str, summary: &str) -> Self {
        Self::create(ItemType::Knowledge, title, summary)
    }

    pub fn new_skill(title: &str, summary: &str) -> Self {
        Self::create(ItemType::Skill, title, summary)
    }

    pub fn new_snippet(title: &str, summary: &str) -> Self {
        Self::create(ItemType::Snippet, title, summary)
    }

    fn create(item_type: ItemType, title: &str, summary: &str) -> Self {
        let now = Utc::now();
        Self {
            id: ItemId::new(),
            item_type,
            title: title.to_string(),
            summary: summary.to_string(),
            content: String::new(),
            tags: Vec::new(),
            source: None,
            created_at: now,
            updated_at: now,
        }
    }

    // ────────────────────────────────────────────────────────
    // 查询方法
    // ────────────────────────────────────────────────────────

    pub fn id(&self) -> &ItemId {
        &self.id
    }

    pub fn item_type(&self) -> ItemType {
        self.item_type
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    // ────────────────────────────────────────────────────────
    // 命令方法（状态变更）
    // ────────────────────────────────────────────────────────

    pub fn set_content(&mut self, content: &str) {
        self.content = content.to_string();
        self.touch();
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
        self.touch();
    }

    pub fn set_summary(&mut self, summary: &str) {
        self.summary = summary.to_string();
        self.touch();
    }

    pub fn add_tag(&mut self, tag: Tag) -> Result<(), DomainError> {
        if self.tags.len() >= 20 {
            return Err(DomainError::TooManyTags);
        }
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.touch();
        }
        Ok(())
    }

    pub fn set_tags(&mut self, tags: Vec<Tag>) -> Result<(), DomainError> {
        if tags.len() > 20 {
            return Err(DomainError::TooManyTags);
        }
        self.tags = tags;
        self.touch();
        Ok(())
    }

    pub fn set_source(&mut self, source: Source) {
        self.source = Some(source);
        self.touch();
    }

    fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    // ────────────────────────────────────────────────────────
    // Builder 方法
    // ────────────────────────────────────────────────────────

    pub fn with_content(mut self, content: &str) -> Self {
        self.content = content.to_string();
        self
    }

    pub fn with_tags(mut self, tags: Vec<Tag>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_id(mut self, id: ItemId) -> Self {
        self.id = id;
        self
    }
}
