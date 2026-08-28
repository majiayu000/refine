//! Facet 提取
//!
//! 从会话内容中提取结构化认知维度 (facets)

use super::SessionMode;
use crate::knowledge::{DocumentId, Item, Source, Tag};

pub(super) const SESSION_PROJECT_SOURCE_PLATFORM: &str = "session-project";

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

提取优先级（高优先级维度应尽量填充）:
1. decisions（最重要）— 明确的技术决策，含选择原因和被拒绝的替代方案
2. bugs_fixed — 修复的 bug，含根因分析
3. patterns — 通用可复用的设计/编码模式
4. knowledge_gained — 次要，仅记录真正新颖的技术知识

维度边界说明（避免重叠）:
- patterns: 通用可复用的编码/设计惯例（如 Builder 模式、错误处理约定），不依赖具体项目
- knowledge_gained: 针对特定技术、API 或领域的新认知（如"了解到 serde 支持 flatten"），不是通用模式
- architecture: 本项目的系统级结构决策（模块划分、服务边界、数据流），与具体项目强绑定；纯讨论不记录，只记录已确定的决策

请以 JSON 格式返回（严格遵守每个字段的条目上限）:
{{
  "session_summary": "一句话概括会话核心内容",
  "cognitive_level": "novice|advanced_beginner|competent|proficient|expert",
  "collaboration_mode": "delegation|pair_programming|review|exploration|teaching|deep_inquiry",
  "decisions": ["做出的技术决策（含原因），最多 5 条"],
  "bugs_fixed": ["修复的 bug（含根因），最多 5 条"],
  "patterns": ["通用可复用的设计/编码模式，最多 3 条"],
  "friction": ["遇到的阻力/困难，最多 3 条"],
  "project_progress": ["项目推进的里程碑，最多 3 条"],
  "questions": ["提出的深度问题，最多 3 条"],
  "knowledge_gained": ["获得的新技术知识（仅记录新颖认知），最多 5 条"],
  "tools_discovered": ["发现或使用的工具/库，最多 3 条"],
  "architecture": ["本项目系统级架构决策（仅已确定的），最多 3 条"],
  "code_artifacts": ["产出的关键代码文件/模块，最多 5 条"]
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

    let preview = trimmed
        .char_indices()
        .find(|&(i, _)| i >= 200)
        .map(|(i, _)| &trimmed[..i])
        .unwrap_or(trimmed);
    Err(format!("无法解析 facet 响应: {}", preview))
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
    facets_to_items_with_mode(facets, document_id, project, SessionMode::Unknown)
}

/// Convert facets to observations while attaching transcript provenance.
pub fn facets_to_items_with_mode(
    facets: &FacetResponse,
    document_id: &DocumentId,
    project: Option<&str>,
    mode: SessionMode,
) -> Vec<Item> {
    facets_to_items_with_mode_and_identity(facets, document_id, project, project, mode)
}

/// Convert facets while retaining the exact pre-normalization project identity.
///
/// `project` remains the backward-compatible display/tag value. The optional
/// identity carries raw cwd evidence through case-normalizing `Tag` storage.
pub fn facets_to_items_with_mode_and_identity(
    facets: &FacetResponse,
    document_id: &DocumentId,
    project: Option<&str>,
    project_identity: Option<&str>,
    mode: SessionMode,
) -> Vec<Item> {
    let mut items = Vec::new();
    let project_source = project_identity
        .map(|identity| Source::new(SESSION_PROJECT_SOURCE_PLATFORM).with_url(identity));

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
    if let Some(source) = &project_source {
        summary_item.set_source(source.clone());
    }

    // 构建标签
    let mut tags = vec![
        Tag::try_new(&facets.cognitive_level),
        Tag::try_new(&facets.collaboration_mode),
        Tag::try_new(mode.as_tag()),
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
        if let Some(source) = &project_source {
            item.set_source(source.clone());
        }
        let mut dtags: Vec<Tag> = Tag::try_new("decision").into_iter().collect();
        dtags.extend(Tag::try_new(mode.as_tag()));
        if let Some(proj) = project {
            dtags.extend(Tag::try_new(proj));
        }
        if let Err(e) = item.set_tags(dtags) {
            tracing::warn!("设置标签失败: {}", e);
        }
        items.push(item);
    }

    // 每个 bug_fixed 单独生成
    for bug in &facets.bugs_fixed {
        let mut item = Item::new_observation(bug, bug);
        item.set_document_id(document_id.clone());
        if let Some(source) = &project_source {
            item.set_source(source.clone());
        }
        let mut btags: Vec<Tag> = Tag::try_new("bugfix").into_iter().collect();
        btags.extend(Tag::try_new(mode.as_tag()));
        if let Some(proj) = project {
            btags.extend(Tag::try_new(proj));
        }
        if let Err(e) = item.set_tags(btags) {
            tracing::warn!("设置标签失败: {}", e);
        }
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

    #[test]
    fn facets_to_items_decision_bugfix_carry_project_tag() {
        let facets = FacetResponse {
            session_summary: "测试".to_string(),
            cognitive_level: "competent".to_string(),
            collaboration_mode: "delegation".to_string(),
            decisions: vec!["用 Rust 重写".to_string()],
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
        let project = "-Users-Lifcc-Desktop-Code-AI-Tools-Harness";
        let items = facets_to_items(&facets, &doc_id, Some(project));

        // decision item (index 1) should carry both "decision" and project tag
        let decision_tags: Vec<&str> = items[1].tags().iter().map(|t| t.as_str()).collect();
        assert!(decision_tags.contains(&"decision"));
        assert!(decision_tags.contains(&"-users-lifcc-desktop-code-ai-tools-harness"));

        // bugfix item (index 2) should carry both "bugfix" and project tag
        let bugfix_tags: Vec<&str> = items[2].tags().iter().map(|t| t.as_str()).collect();
        assert!(bugfix_tags.contains(&"bugfix"));
        assert!(bugfix_tags.contains(&"-users-lifcc-desktop-code-ai-tools-harness"));
        assert!(items.iter().all(|item| item
            .source()
            .is_some_and(|source| source.platform == SESSION_PROJECT_SOURCE_PLATFORM
                && source.url.as_deref() == Some(project))));
    }

    #[test]
    fn facets_to_items_attach_mode_to_every_observation() {
        let facets = FacetResponse {
            session_summary: "测试".to_string(),
            cognitive_level: "competent".to_string(),
            collaboration_mode: "review".to_string(),
            decisions: vec!["保留来源".to_string()],
            bugs_fixed: vec!["修复标签".to_string()],
            patterns: Vec::new(),
            friction: Vec::new(),
            project_progress: Vec::new(),
            questions: Vec::new(),
            knowledge_gained: Vec::new(),
            tools_discovered: Vec::new(),
            architecture: Vec::new(),
            code_artifacts: Vec::new(),
        };

        let items = facets_to_items_with_mode(
            &facets,
            &DocumentId::new(),
            Some("refine"),
            SessionMode::Unattended,
        );
        assert!(items.iter().all(|item| item
            .tags()
            .iter()
            .any(|tag| tag.as_str() == "session_mode_unattended")));
    }
}
