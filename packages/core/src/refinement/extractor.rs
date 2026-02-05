//! 知识提炼器
//!
//! 核心提炼逻辑

use crate::error::{DomainError, DomainResult};
use crate::knowledge::{Item, ItemType, Tag};
use crate::refinement::conversation::Conversation;
use crate::refinement::policy::ExtractionPolicy;
use serde::Deserialize;

/// 提炼结果
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub items: Vec<Item>,
    pub raw_response: String,
}

/// LLM 响应的 JSON 结构
#[derive(Debug, Deserialize)]
pub struct LlmResponse {
    pub items: Vec<LlmItem>,
}

#[derive(Debug, Deserialize)]
pub struct LlmItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub tags: Vec<String>,
}

/// 提炼器
pub struct Extractor {
    policy: ExtractionPolicy,
}

impl Extractor {
    pub fn new(policy: ExtractionPolicy) -> Self {
        Self { policy }
    }

    pub fn with_default_policy() -> Self {
        Self::new(ExtractionPolicy::default())
    }

    /// 从 LLM 响应解析结果
    pub fn parse_response(
        &self,
        response: &str,
        conversation: &Conversation,
    ) -> DomainResult<ExtractionResult> {
        // 提取 JSON 部分
        let json_str = Self::extract_json(response)?;

        // 解析 JSON
        let llm_response: LlmResponse = serde_json::from_str(&json_str)
            .map_err(|e| DomainError::Extraction(format!("JSON 解析失败: {}", e)))?;

        // 转换为 Item
        let items = self.convert_items(&llm_response.items, conversation)?;

        Ok(ExtractionResult {
            items,
            raw_response: response.to_string(),
        })
    }

    /// 从响应中提取 JSON
    fn extract_json(response: &str) -> DomainResult<String> {
        // 尝试找到 JSON 块
        if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                return Ok(response[start..=end].to_string());
            }
        }

        // 尝试 code block
        if response.contains("```json") {
            let parts: Vec<&str> = response.split("```json").collect();
            if parts.len() > 1 {
                if let Some(end) = parts[1].find("```") {
                    return Ok(parts[1][..end].trim().to_string());
                }
            }
        }

        Err(DomainError::Extraction("未找到有效的 JSON".into()))
    }

    /// 转换 LLM 输出为 Item
    fn convert_items(
        &self,
        llm_items: &[LlmItem],
        _conversation: &Conversation,
    ) -> DomainResult<Vec<Item>> {
        let mut items = Vec::new();

        for llm_item in llm_items {
            let item_type = match llm_item.item_type.as_str() {
                "knowledge" => ItemType::Knowledge,
                "skill" => ItemType::Skill,
                "snippet" => ItemType::Snippet,
                _ => continue,
            };

            // 检查策略是否允许
            if !self.policy.should_extract(item_type) {
                continue;
            }

            // 创建 Item
            let mut item = match item_type {
                ItemType::Knowledge => Item::new_knowledge(&llm_item.title, &llm_item.summary),
                ItemType::Skill => Item::new_skill(&llm_item.title, &llm_item.summary),
                ItemType::Snippet => Item::new_snippet(&llm_item.title, &llm_item.summary),
            };

            // 设置内容
            item.set_content(&llm_item.content);

            // 设置标签
            let tags: Vec<Tag> = llm_item
                .tags
                .iter()
                .take(self.policy.max_tags)
                .filter_map(|t| Tag::try_new(t))
                .collect();

            if let Err(e) = item.set_tags(tags) {
                tracing::warn!("设置标签失败: {}", e);
            }

            items.push(item);
        }

        Ok(items)
    }

    /// 获取策略
    pub fn policy(&self) -> &ExtractionPolicy {
        &self.policy
    }
}
