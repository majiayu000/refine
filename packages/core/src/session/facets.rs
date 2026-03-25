//! Facet 提取
//!
//! 从会话内容中提取结构化认知维度 (facets)

use crate::knowledge::{DocumentId, Item, Tag};

/// Facet 提取结果
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FacetResponse {
    #[serde(default)]
    pub session_summary: String,
    #[serde(default)]
    pub cognitive_level: String,
    #[serde(default)]
    pub collaboration_mode: String,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub bugs_fixed: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub friction: Vec<String>,
    #[serde(default)]
    pub project_progress: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub knowledge_gained: Vec<String>,
    #[serde(default)]
    pub tools_discovered: Vec<String>,
    #[serde(default)]
    pub architecture: Vec<String>,
    #[serde(default)]
    pub code_artifacts: Vec<String>,
}

/// 构建 facet 提取的系统 prompt
pub const FACET_SYSTEM_PROMPT: &str =
    "你是认知分析助手。分析编程会话，提取结构化观测。严格返回 JSON，不要输出额外说明。";

/// 构建 facet 提取 prompt
pub fn build_facet_prompt(session_content: &str) -> String {
    format!(
        r#"分析以下 AI 编程会话，提取结构化认知观测。

会话内容:
{session_content}

请以 JSON 格式返回:
{{
  "session_summary": "一句话概括会话核心内容",
  "cognitive_level": "novice|advanced_beginner|competent|proficient|expert",
  "collaboration_mode": "delegation|pair_programming|review|exploration|teaching|deep_inquiry",
  "decisions": ["做出的技术决策（含原因）"],
  "bugs_fixed": ["修复的 bug（含根因）"],
  "patterns": ["使用或发现的设计模式/编码模式"],
  "friction": ["遇到的阻力/困难"],
  "project_progress": ["项目推进的里程碑"],
  "questions": ["提出的深度问题"],
  "knowledge_gained": ["获得的新知识"],
  "tools_discovered": ["发现或使用的工具/库"],
  "architecture": ["架构设计相关的讨论或决策"],
  "code_artifacts": ["产出的关键代码文件/模块"]
}}

每个数组中的条目应为简洁的描述性文本。空数组表示该维度无观测。"#
    )
}

/// 解析 LLM 返回的 facet JSON 响应
pub fn parse_facet_response(response: &str) -> Result<FacetResponse, String> {
    // 复用 extractor 的 JSON 候选提取策略
    let trimmed = response.trim();

    // 尝试直接解析
    if let Ok(parsed) = serde_json::from_str::<FacetResponse>(trimmed) {
        return Ok(parsed);
    }

    // 尝试从 markdown code fence 提取
    if let Some(json_str) = extract_json_from_fence(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<FacetResponse>(&json_str) {
            return Ok(parsed);
        }
    }

    // 尝试找第一个平衡的 JSON 对象
    if let Some(start) = trimmed.find('{') {
        let candidate = &trimmed[start..];
        if let Ok(parsed) = serde_json::from_str::<FacetResponse>(candidate) {
            return Ok(parsed);
        }
    }

    Err(format!(
        "无法解析 facet 响应: {}",
        &trimmed[..trimmed.len().min(200)]
    ))
}

fn extract_json_from_fence(text: &str) -> Option<String> {
    let blocks: Vec<&str> = text.split("```").collect();
    for idx in (1..blocks.len()).step_by(2) {
        let block = blocks[idx].trim();
        let mut lines = block.lines();
        let first = lines.next().unwrap_or_default().trim().to_lowercase();
        let body = if first == "json" || first == "javascript" {
            lines.collect::<Vec<_>>().join("\n")
        } else {
            block.to_string()
        };
        let body = body.trim().to_string();
        if !body.is_empty() {
            return Some(body);
        }
    }
    None
}

/// 将 facet 响应转换为 Observation Items
pub fn facets_to_items(
    facets: &FacetResponse,
    document_id: &DocumentId,
    project: Option<&str>,
) -> Vec<Item> {
    let mut items = Vec::new();

    // 宏观标注作为一个综合 observation
    let mut summary_item = Item::new_observation(&facets.session_summary, &facets.session_summary);
    let mut content = format!(
        "认知水平: {}\n协作模式: {}",
        facets.cognitive_level, facets.collaboration_mode
    );
    if !facets.decisions.is_empty() {
        content.push_str(&format!("\n\n决策:\n- {}", facets.decisions.join("\n- ")));
    }
    if !facets.bugs_fixed.is_empty() {
        content.push_str(&format!(
            "\n\nBug 修复:\n- {}",
            facets.bugs_fixed.join("\n- ")
        ));
    }
    if !facets.patterns.is_empty() {
        content.push_str(&format!("\n\n模式:\n- {}", facets.patterns.join("\n- ")));
    }
    if !facets.friction.is_empty() {
        content.push_str(&format!("\n\n阻力:\n- {}", facets.friction.join("\n- ")));
    }
    if !facets.project_progress.is_empty() {
        content.push_str(&format!(
            "\n\n进展:\n- {}",
            facets.project_progress.join("\n- ")
        ));
    }
    if !facets.questions.is_empty() {
        content.push_str(&format!("\n\n问题:\n- {}", facets.questions.join("\n- ")));
    }
    if !facets.knowledge_gained.is_empty() {
        content.push_str(&format!(
            "\n\n知识:\n- {}",
            facets.knowledge_gained.join("\n- ")
        ));
    }
    if !facets.tools_discovered.is_empty() {
        content.push_str(&format!(
            "\n\n工具:\n- {}",
            facets.tools_discovered.join("\n- ")
        ));
    }
    if !facets.architecture.is_empty() {
        content.push_str(&format!(
            "\n\n架构:\n- {}",
            facets.architecture.join("\n- ")
        ));
    }
    if !facets.code_artifacts.is_empty() {
        content.push_str(&format!(
            "\n\n代码产出:\n- {}",
            facets.code_artifacts.join("\n- ")
        ));
    }
    summary_item.set_content(&content);
    summary_item.set_document_id(document_id.clone());

    // 构建标签
    let mut tags = vec![
        Tag::try_new(&facets.cognitive_level),
        Tag::try_new(&facets.collaboration_mode),
    ];
    if let Some(proj) = project {
        tags.push(Tag::try_new(proj));
    }
    let tags: Vec<Tag> = tags.into_iter().flatten().collect();
    if let Err(e) = summary_item.set_tags(tags) {
        tracing::warn!("设置标签失败: {}", e);
    }

    items.push(summary_item);

    // 每个 decision 单独生成一个 observation
    for decision in &facets.decisions {
        let mut item = Item::new_observation(decision, decision);
        item.set_document_id(document_id.clone());
        let tag = Tag::try_new("decision").into_iter().collect();
        let _ = item.set_tags(tag);
        items.push(item);
    }

    // 每个 bug_fixed 单独生成
    for bug in &facets.bugs_fixed {
        let mut item = Item::new_observation(bug, bug);
        item.set_document_id(document_id.clone());
        let tag = Tag::try_new("bugfix").into_iter().collect();
        let _ = item.set_tags(tag);
        items.push(item);
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::ItemType;

    #[test]
    fn parse_facet_response_handles_valid_json() {
        let json = r#"{
            "session_summary": "实现了会话解析功能",
            "cognitive_level": "proficient",
            "collaboration_mode": "pair_programming",
            "decisions": ["选择 serde_json 解析"],
            "bugs_fixed": [],
            "patterns": ["Builder 模式"],
            "friction": [],
            "project_progress": ["完成解析器"],
            "questions": [],
            "knowledge_gained": ["JSONL 格式"],
            "tools_discovered": [],
            "architecture": [],
            "code_artifacts": ["parser.rs"]
        }"#;

        let result = parse_facet_response(json).unwrap();
        assert_eq!(result.session_summary, "实现了会话解析功能");
        assert_eq!(result.cognitive_level, "proficient");
        assert_eq!(result.decisions.len(), 1);
    }

    #[test]
    fn parse_facet_response_handles_code_fence() {
        let response = r#"Here's the analysis:

```json
{
    "session_summary": "test",
    "cognitive_level": "novice",
    "collaboration_mode": "delegation",
    "decisions": [],
    "bugs_fixed": [],
    "patterns": [],
    "friction": [],
    "project_progress": [],
    "questions": [],
    "knowledge_gained": [],
    "tools_discovered": [],
    "architecture": [],
    "code_artifacts": []
}
```"#;

        let result = parse_facet_response(response).unwrap();
        assert_eq!(result.session_summary, "test");
    }

    #[test]
    fn facets_to_items_creates_observation_items() {
        let facets = FacetResponse {
            session_summary: "测试会话".to_string(),
            cognitive_level: "proficient".to_string(),
            collaboration_mode: "pair_programming".to_string(),
            decisions: vec!["选择 A 方案".to_string()],
            bugs_fixed: vec!["修复空指针".to_string()],
            patterns: Vec::new(),
            friction: Vec::new(),
            project_progress: Vec::new(),
            questions: Vec::new(),
            knowledge_gained: Vec::new(),
            tools_discovered: Vec::new(),
            architecture: Vec::new(),
            code_artifacts: Vec::new(),
        };
        let doc_id = DocumentId::new();
        let items = facets_to_items(&facets, &doc_id, Some("my-project"));

        // 1 summary + 1 decision + 1 bugfix = 3
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|i| i.item_type() == ItemType::Observation));
        assert_eq!(items[0].title(), "测试会话");
    }
}
