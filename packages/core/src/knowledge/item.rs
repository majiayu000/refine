//! Item 聚合根
//!
//! 知识片段的核心实体，控制内部一致性

use crate::error::DomainError;
use crate::knowledge::types::{DocumentId, ItemId, ItemType, Source, Tag};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 从持久化存储还原 Item 的参数
pub struct RestoreParams {
    pub id: ItemId,
    pub item_type: ItemType,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub tags: Vec<Tag>,
    pub source: Option<Source>,
    pub document_id: Option<DocumentId>,
    pub excerpt: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

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
    document_id: Option<DocumentId>,
    excerpt: Option<String>,
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

    pub fn new_observation(title: &str, summary: &str) -> Self {
        Self::create(ItemType::Observation, title, summary)
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
            document_id: None,
            excerpt: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// 从持久化存储还原实体（不触发业务时间更新）
    pub fn restore(params: RestoreParams) -> Result<Self, DomainError> {
        if params.tags.len() > 20 {
            return Err(DomainError::TooManyTags);
        }

        Ok(Self {
            id: params.id,
            item_type: params.item_type,
            title: params.title,
            summary: params.summary,
            content: params.content,
            tags: params.tags,
            source: params.source,
            document_id: params.document_id,
            excerpt: params.excerpt,
            created_at: params.created_at,
            updated_at: params.updated_at,
        })
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

    pub fn document_id(&self) -> Option<&DocumentId> {
        self.document_id.as_ref()
    }

    pub fn excerpt(&self) -> Option<&str> {
        self.excerpt.as_deref()
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

    pub fn set_document_id(&mut self, id: DocumentId) {
        self.document_id = Some(id);
        self.touch();
    }

    pub fn set_excerpt(&mut self, excerpt: &str) {
        self.excerpt = Some(excerpt.to_string());
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
