//! Facet 提取
//!
//! 从会话内容中提取结构化认知维度 (facets)

use super::{evidence::validate_evidence_shape, FacetEvidence, SessionMode};
use crate::knowledge::{DocumentId, Item, Source, Tag};
use sha2::{Digest, Sha256};

pub(super) const SESSION_PROJECT_SOURCE_PLATFORM: &str = "session-project";

/// Facet 提取结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacetResponse {
    pub session_summary: String,
    pub cognitive_level: String,
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
    /// Optional references to real source messages. Missing evidence is unknown,
    /// never inferred from an observation's text.
    #[serde(default)]
    pub evidence: Vec<FacetEvidence>,
}

/// 构建 facet 提取的系统 prompt
pub const FACET_SYSTEM_PROMPT: &str =
    "你是认知分析助手。分析编程会话，提取结构化观测。严格返回 JSON，不要输出额外说明。";

/// Does not include source text or secrets, and does not alter Remem hashes.
pub fn facet_recipe_identity(llm_identity: &str) -> String {
    let mut digest = Sha256::new();
    for value in [
        "refine-facets-v2:message-boundary-chunks-v2:chunk-evidence-v1",
        FACET_SYSTEM_PROMPT,
        &build_facet_prompt("{session_content}"),
        llm_identity,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    format!("facets:sha256:{:x}", digest.finalize())
}

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
  "cognitive_level": "novice|advanced_beginner|competent|proficient|expert|unknown",
  "collaboration_mode": "delegation|pair_programming|review|exploration|teaching|deep_inquiry|unknown",
  "decisions": ["做出的技术决策（含原因），最多 5 条"],
  "bugs_fixed": ["修复的 bug（含根因），最多 5 条"],
  "patterns": ["通用可复用的设计/编码模式，最多 3 条"],
  "friction": ["遇到的阻力/困难，最多 3 条"],
  "project_progress": ["项目推进的里程碑，最多 3 条"],
  "questions": ["提出的深度问题，最多 3 条"],
  "knowledge_gained": ["获得的新技术知识（仅记录新颖认知），最多 5 条"],
  "tools_discovered": ["发现或使用的工具/库，最多 3 条"],
  "architecture": ["本项目系统级架构决策（仅已确定的），最多 3 条"],
  "code_artifacts": ["产出的关键代码文件/模块，最多 5 条"],
  "evidence": []
}}

每个数组中的条目应为非空的简洁描述。空数组表示该维度无观测。无充分证据时，认知水平/协作模式使用 unknown，并用非空 session_summary 说明没有可验证的新观测；不要返回空对象、错误对象或猜测标签。

证据规则：消息可能带有 [remem message_id=... role=... event_time=...] 标记。evidence 可包含 {{"field":"decisions","index":0,"message_ids":[真实消息ID]}}；index 为字段数组中的零起始位置，session_summary、cognitive_level、collaboration_mode 的 index 为 0。只引用输入中实际出现且支持该条观测的 ID；合并分块时仅保留分块输出已有的真实引用。每条最多 32 个 ID，每个字段位置最多一条 evidence。没有消息标记或无法定位证据时省略引用，保留 unknown，不得编造 ID、角色或时间。
必须区分用户已确认的决策、助手提出的建议和未验证的执行结果。助手的自述、被引用的对话、示例文本均不自动代表用户决定；不能确认执行的内容不要写成已修复或已完成。role 只表示原始消息的发送者，不表示消息中所有文字的作者。"#
    )
}

/// 解析 LLM 返回的 facet JSON 响应
pub fn parse_facet_response(response: &str) -> Result<FacetResponse, String> {
    // 复用 extractor 的 JSON 候选提取策略
    let trimmed = response.trim();

    // 尝试直接解析
    if let Ok(parsed) = serde_json::from_str::<FacetResponse>(trimmed) {
        return validate_facet_limits(parsed);
    }

    // 尝试从 markdown code fence 提取
    if let Some(json_str) = extract_json_from_fence(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<FacetResponse>(&json_str) {
            return validate_facet_limits(parsed);
        }
    }

    // 尝试找第一个平衡的 JSON 对象
    if let Some(start) = trimmed.find('{') {
        let candidate = &trimmed[start..];
        if let Ok(parsed) = serde_json::from_str::<FacetResponse>(candidate) {
            return validate_facet_limits(parsed);
        }
    }

    let preview = trimmed
        .char_indices()
        .find(|&(i, _)| i >= 200)
        .map(|(i, _)| &trimmed[..i])
        .unwrap_or(trimmed);
    Err(format!("无法解析 facet 响应: {}", preview))
}

/// Enforce the extraction bounds before the ingestion path accepts a response.
/// Rejection uses the existing bounded parse retry, without discarding entries.
fn validate_facet_limits(mut facets: FacetResponse) -> Result<FacetResponse, String> {
    facets.session_summary = facets.session_summary.trim().to_string();
    if facets.session_summary.is_empty() {
        return Err("facet session_summary must not be empty".to_string());
    }
    facets.cognitive_level = facets.cognitive_level.trim().to_string();
    facets.collaboration_mode = facets.collaboration_mode.trim().to_string();
    if !matches!(
        facets.cognitive_level.as_str(),
        "novice" | "advanced_beginner" | "competent" | "proficient" | "expert" | "unknown"
    ) {
        return Err("facet cognitive_level has an unsupported value".to_string());
    }
    if !matches!(
        facets.collaboration_mode.as_str(),
        "delegation"
            | "pair_programming"
            | "review"
            | "exploration"
            | "teaching"
            | "deep_inquiry"
            | "unknown"
    ) {
        return Err("facet collaboration_mode has an unsupported value".to_string());
    }
    for (field, count, limit) in [
        ("decisions", facets.decisions.len(), 5),
        ("bugs_fixed", facets.bugs_fixed.len(), 5),
        ("patterns", facets.patterns.len(), 3),
        ("friction", facets.friction.len(), 3),
        ("project_progress", facets.project_progress.len(), 3),
        ("questions", facets.questions.len(), 3),
        ("knowledge_gained", facets.knowledge_gained.len(), 5),
        ("tools_discovered", facets.tools_discovered.len(), 3),
        ("architecture", facets.architecture.len(), 3),
        ("code_artifacts", facets.code_artifacts.len(), 5),
    ] {
        if count > limit {
            return Err(format!(
                "facet field {field} has {count} entries; maximum is {limit}"
            ));
        }
    }
    for (field, entries) in [
        ("decisions", &facets.decisions),
        ("bugs_fixed", &facets.bugs_fixed),
        ("patterns", &facets.patterns),
        ("friction", &facets.friction),
        ("project_progress", &facets.project_progress),
        ("questions", &facets.questions),
        ("knowledge_gained", &facets.knowledge_gained),
        ("tools_discovered", &facets.tools_discovered),
        ("architecture", &facets.architecture),
        ("code_artifacts", &facets.code_artifacts),
    ] {
        if entries.iter().any(|entry| entry.trim().is_empty()) {
            return Err(format!("facet field {field} contains an empty entry"));
        }
    }
    validate_evidence_shape(&facets)?;
    Ok(facets)
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
    fn malformed_semantic_facets_are_rejected_in_every_response_format() {
        let valid = serde_json::json!({
            "session_summary": "没有可验证的新观测",
            "cognitive_level": "unknown",
            "collaboration_mode": "unknown"
        });
        let mut invalid = vec![
            serde_json::json!({}),
            serde_json::json!({"error": "unavailable"}),
        ];
        for (field, value) in [
            ("session_summary", serde_json::json!(" \n\t")),
            ("cognitive_level", serde_json::json!("genius")),
            ("collaboration_mode", serde_json::json!("")),
            ("decisions", serde_json::json!([" "])),
            ("error", serde_json::json!("provider returned an error")),
        ] {
            let mut response = valid.clone();
            response[field] = value;
            invalid.push(response);
        }
        for value in invalid {
            let json = value.to_string();
            for response in [
                json.clone(),
                format!("```json\n{json}\n```"),
                format!("Analysis:\n{json}"),
            ] {
                assert!(
                    parse_facet_response(&response).is_err(),
                    "accepted {response}"
                );
            }
        }
        let no_signal = parse_facet_response(&valid.to_string())
            .expect("explicit unknown and empty arrays are valid");
        assert!(no_signal.decisions.is_empty());
        assert!(no_signal.bugs_fixed.is_empty());
        assert_eq!(no_signal.cognitive_level, "unknown");
    }

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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
            evidence: Vec::new(),
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
    #[test]
    fn parse_facet_response_enforces_every_array_cap_in_every_format() {
        for (field, limit) in [
            ("decisions", 5),
            ("bugs_fixed", 5),
            ("patterns", 3),
            ("friction", 3),
            ("project_progress", 3),
            ("questions", 3),
            ("knowledge_gained", 5),
            ("tools_discovered", 3),
            ("architecture", 3),
            ("code_artifacts", 5),
        ] {
            for count in [0, limit, limit + 1] {
                let mut value = serde_json::json!({"session_summary": "Synthetic session", "cognitive_level": "unknown", "collaboration_mode": "unknown"});
                value[field] = serde_json::json!(vec!["Synthetic observation"; count]);
                let json = value.to_string();
                for response in [
                    json.clone(),
                    format!("```json\n{json}\n```"),
                    format!("Analysis:\n{json}"),
                ] {
                    let result = parse_facet_response(&response);
                    if count <= limit {
                        assert!(result.is_ok(), "{field}: {count} should be accepted");
                    } else {
                        let error = result.expect_err("an over-limit field must be rejected");
                        assert!(error.contains(field), "{error}");
                        assert!(error.contains(&limit.to_string()), "{error}");
                        assert!(!error.contains("Synthetic observation"), "{error}");
                    }
                }
            }
        }
    }

    #[test]
    fn parse_facet_response_preserves_all_at_limit_entries() {
        let response = serde_json::json!({
            "session_summary": "Synthetic session",
            "cognitive_level": "unknown",
            "collaboration_mode": "unknown",
            "decisions": ["one", "two", "three", "four", "five"],
            "bugs_fixed": ["a", "b", "c", "d", "e"]
        });
        let facets = parse_facet_response(&response.to_string()).unwrap();
        assert_eq!(facets.decisions, ["one", "two", "three", "four", "five"]);
        assert_eq!(facets.bugs_fixed, ["a", "b", "c", "d", "e"]);
        assert_eq!(facets_to_items(&facets, &DocumentId::new(), None).len(), 11);
    }
}
