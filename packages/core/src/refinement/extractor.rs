//! 知识提炼器
//!
//! 核心提炼逻辑

use crate::error::{DomainError, DomainResult};
use crate::knowledge::{Item, ItemType, Tag};
use crate::refinement::conversation::Conversation;
use crate::refinement::policy::ExtractionPolicy;
use serde::Deserialize;
use std::collections::HashSet;

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
    #[serde(default)]
    pub excerpt: Option<String>,
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
        let llm_response = Self::parse_llm_response(response)?;

        // 转换为 Item
        let items = self.convert_items(&llm_response.items, conversation)?;

        Ok(ExtractionResult {
            items,
            raw_response: response.to_string(),
        })
    }

    fn parse_llm_response(response: &str) -> DomainResult<LlmResponse> {
        let candidates = Self::extract_json_candidates(response);
        if candidates.is_empty() {
            return Err(DomainError::Extraction("未找到有效的 JSON".into()));
        }

        let mut last_error = None;
        for candidate in candidates {
            let value = match serde_json::from_str::<serde_json::Value>(&candidate) {
                Ok(value) => value,
                Err(err) => {
                    last_error = Some(format!("JSON 语法错误: {}", err));
                    continue;
                }
            };

            let Some(normalized) = Self::normalize_llm_value(value) else {
                last_error = Some("JSON 结构不符合要求: 缺少 items 或合法 item 字段".to_string());
                continue;
            };

            match serde_json::from_value::<LlmResponse>(normalized) {
                Ok(parsed) => return Ok(parsed),
                Err(err) => {
                    last_error = Some(format!("JSON 结构解析失败: {}", err));
                }
            }
        }

        let message = last_error
            .map(|e| format!("JSON 解析失败: {}", e))
            .unwrap_or_else(|| "JSON 解析失败: 未知错误".to_string());
        Err(DomainError::Extraction(message))
    }

    fn normalize_llm_value(value: serde_json::Value) -> Option<serde_json::Value> {
        match value {
            serde_json::Value::Object(mut map) => {
                if let Some(items) = map.remove("items") {
                    return match items {
                        serde_json::Value::Array(_) => Some(serde_json::json!({ "items": items })),
                        serde_json::Value::Object(_) => {
                            Some(serde_json::json!({ "items": [items] }))
                        }
                        _ => None,
                    };
                }

                let is_single_item = map.contains_key("type")
                    && map.contains_key("title")
                    && map.contains_key("summary")
                    && map.contains_key("content");
                if is_single_item {
                    return Some(serde_json::json!({
                        "items": [serde_json::Value::Object(map)]
                    }));
                }
                None
            }
            serde_json::Value::Array(items) => Some(serde_json::json!({ "items": items })),
            _ => None,
        }
    }

    fn extract_json_candidates(response: &str) -> Vec<String> {
        let mut candidates = Vec::new();

        let trimmed = response.trim();
        if !trimmed.is_empty() {
            candidates.push(trimmed.to_string());
        }

        // 优先尝试 markdown code fence 内部内容。
        let blocks: Vec<&str> = response.split("```").collect();
        for idx in (1..blocks.len()).step_by(2) {
            let block = blocks[idx].trim();
            if block.is_empty() {
                continue;
            }

            let mut lines = block.lines();
            let first_line = lines.next().unwrap_or_default().trim();
            let fence_json = matches!(
                first_line.to_ascii_lowercase().as_str(),
                "json" | "javascript" | "js"
            );
            let body = if fence_json {
                lines.collect::<Vec<_>>().join("\n")
            } else {
                block.to_string()
            };
            let body = body.trim();
            if !body.is_empty() {
                candidates.push(body.to_string());
            }
        }

        // 扫描所有平衡的大括号对象，避免简单 first/last 截断误伤。
        for (start, ch) in response.char_indices() {
            if ch != '{' {
                continue;
            }
            if let Some(candidate) = Self::balanced_object_at(response, start) {
                candidates.push(candidate);
            }
        }

        let mut seen = HashSet::new();
        candidates
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .filter(|v| seen.insert(v.clone()))
            .collect()
    }

    fn balanced_object_at(input: &str, start: usize) -> Option<String> {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;

        for (offset, ch) in input[start..].char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }

                match ch {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    _ => {}
                }
                continue;
            }

            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        let end = start + offset + ch.len_utf8();
                        return Some(input[start..end].to_string());
                    }
                }
                _ => {}
            }
        }
        None
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

            // 设置原文引用
            if let Some(excerpt) = &llm_item.excerpt {
                let trimmed = excerpt.trim();
                if !trimmed.is_empty() {
                    item.set_excerpt(trimmed);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refinement::Conversation;

    fn sample_conversation() -> Conversation {
        Conversation::parse("Human: 你好\nAssistant: 好的").unwrap()
    }

    #[test]
    fn parse_response_handles_json_code_fence() {
        let response = r#"下面是结果：

```json
{
  "items": [
    {
      "type": "knowledge",
      "title": "Rust HTTP 服务",
      "summary": "用 axum 即可",
      "content": "推荐 axum + tokio",
      "tags": ["rust", "axum"]
    }
  ]
}
```
"#;

        let extractor = Extractor::with_default_policy();
        let parsed = extractor
            .parse_response(response, &sample_conversation())
            .unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title(), "Rust HTTP 服务");
    }

    #[test]
    fn parse_response_ignores_invalid_candidate_and_uses_valid_one() {
        let response = r#"{
  "items": [
    {"type":"knowledge","title":"bad","summary":"bad","content":"broken "quote","tags":[]}
  ]
}

{
  "items": [
    {
      "type": "skill",
      "title": "有效结果",
      "summary": "这是正确 JSON",
      "content": "最终应当解析这一段",
      "tags": ["ok"]
    }
  ]
}"#;

        let extractor = Extractor::with_default_policy();
        let parsed = extractor
            .parse_response(response, &sample_conversation())
            .unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title(), "有效结果");
    }

    #[test]
    fn parse_response_accepts_single_item_object_without_items_wrapper() {
        let response = r#"{
  "type": "knowledge",
  "title": "单对象结构",
  "summary": "没有 items 包裹也应支持",
  "content": "自动归一化为 items 数组",
  "tags": ["compat"]
}"#;

        let extractor = Extractor::with_default_policy();
        let parsed = extractor
            .parse_response(response, &sample_conversation())
            .unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title(), "单对象结构");
    }

    #[test]
    fn parse_response_accepts_array_root_as_items() {
        let response = r#"[
  {
    "type": "skill",
    "title": "数组根结构",
    "summary": "数组也应支持",
    "content": "自动归一化为 items",
    "tags": ["array"]
  }
]"#;

        let extractor = Extractor::with_default_policy();
        let parsed = extractor
            .parse_response(response, &sample_conversation())
            .unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title(), "数组根结构");
    }
}
