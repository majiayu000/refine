//! Document 聚合根
//!
//! 原文的一等公民载体，存储完整源内容

use crate::knowledge::types::DocumentId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 从持久化存储还原 Document 的参数
pub struct RestoreDocumentParams {
    pub id: DocumentId,
    pub title: Option<String>,
    pub raw_content: String,
    pub source: String,
    pub url: String,
    pub source_version: Option<String>,
    pub captured_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 文档 - 原文聚合根
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    id: DocumentId,
    title: Option<String>,
    raw_content: String,
    source: String,
    url: String,
    #[serde(default)]
    source_version: Option<String>,
    captured_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Document {
    pub fn new(source: &str, raw_content: &str) -> Self {
        let now = Utc::now();
        Self {
            id: DocumentId::new(),
            title: None,
            raw_content: raw_content.to_string(),
            source: source.to_string(),
            url: String::new(),
            source_version: None,
            captured_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn restore(params: RestoreDocumentParams) -> Self {
        Self {
            id: params.id,
            title: params.title,
            raw_content: params.raw_content,
            source: params.source,
            url: params.url,
            source_version: params.source_version,
            captured_at: params.captured_at,
            created_at: params.created_at,
            updated_at: params.updated_at,
        }
    }

    // 查询方法

    pub fn id(&self) -> &DocumentId {
        &self.id
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn raw_content(&self) -> &str {
        &self.raw_content
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn source_version(&self) -> Option<&str> {
        self.source_version.as_deref()
    }
    pub fn captured_at(&self) -> DateTime<Utc> {
        self.captured_at
    }
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    // 命令方法

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_string());
        self.updated_at = Utc::now();
    }

    pub fn set_url(&mut self, url: &str) {
        self.url = url.to_string();
        self.updated_at = Utc::now();
    }

    pub fn set_source_version(&mut self, source_version: Option<&str>) {
        self.source_version = source_version.map(ToOwned::to_owned);
        self.updated_at = Utc::now();
    }

    pub fn set_captured_at(&mut self, captured_at: DateTime<Utc>) {
        self.captured_at = captured_at;
        self.updated_at = Utc::now();
    }
}
