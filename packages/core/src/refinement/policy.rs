//! 提炼策略
//!
//! 定义如何从对话中提取知识

use crate::knowledge::ItemType;

/// 提炼策略配置
#[derive(Debug, Clone)]
pub struct ExtractionPolicy {
    /// 是否提取知识卡片
    pub extract_knowledge: bool,
    /// 是否提取技能
    pub extract_skills: bool,
    /// 是否提取代码片段
    pub extract_snippets: bool,
    /// 最大标签数
    pub max_tags: usize,
    /// 摘要最大长度
    pub max_summary_len: usize,
}

impl Default for ExtractionPolicy {
    fn default() -> Self {
        Self {
            extract_knowledge: true,
            extract_skills: true,
            extract_snippets: true,
            max_tags: 5,
            max_summary_len: 200,
        }
    }
}

impl ExtractionPolicy {
    /// 只提取知识
    pub fn knowledge_only() -> Self {
        Self {
            extract_knowledge: true,
            extract_skills: false,
            extract_snippets: false,
            ..Default::default()
        }
    }

    /// 只提取代码
    pub fn snippets_only() -> Self {
        Self {
            extract_knowledge: false,
            extract_skills: false,
            extract_snippets: true,
            ..Default::default()
        }
    }

    /// 判断是否应该提取指定类型
    pub fn should_extract(&self, item_type: ItemType) -> bool {
        match item_type {
            ItemType::Knowledge => self.extract_knowledge,
            ItemType::Skill => self.extract_skills,
            ItemType::Snippet => self.extract_snippets,
        }
    }
}

/// 提炼 Prompt 模板
pub struct PromptTemplate;

impl PromptTemplate {
    /// 生成提炼 prompt
    pub fn extraction_prompt(conversation: &str, policy: &ExtractionPolicy) -> String {
        let mut types = Vec::new();
        if policy.extract_knowledge {
            types.push("知识卡片 (knowledge)");
        }
        if policy.extract_skills {
            types.push("可执行技能 (skill)");
        }
        if policy.extract_snippets {
            types.push("代码片段 (snippet)");
        }

        format!(
            r#"分析以下对话，提取有价值的内容。

提取类型: {}
每个提取项最多 {} 个标签
摘要最长 {} 字

对话内容:
{}

请以 JSON 格式返回:
{{
  "items": [
    {{
      "type": "knowledge|skill|snippet",
      "title": "标题",
      "summary": "简短摘要",
      "content": "详细内容",
      "tags": ["标签1", "标签2"]
    }}
  ]
}}"#,
            types.join(", "),
            policy.max_tags,
            policy.max_summary_len,
            conversation
        )
    }
}
