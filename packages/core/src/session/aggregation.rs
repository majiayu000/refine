//! L1-L3 聚合分析
//!
//! 从 Observation Items 中计算宏观认知指标

use crate::knowledge::{Item, ItemType};
use std::collections::HashMap;

/// L1-L3 聚合报告
#[derive(Debug, Clone)]
pub struct AggregationReport {
    pub total_sessions: usize,
    pub l1: L1CognitiveEvolution,
    pub l2: L2StrategicPosition,
    pub l3: L3CollaborationEfficiency,
}

/// L1: 认知演进
#[derive(Debug, Clone)]
pub struct L1CognitiveEvolution {
    /// 认知水平分布: novice/advanced_beginner/competent/proficient/expert → count
    pub cognitive_levels: HashMap<String, usize>,
    /// 提问深度趋势（问题总数）
    pub total_questions: usize,
    /// 知识获取数
    pub knowledge_gained_count: usize,
}

/// L2: 战略定位
#[derive(Debug, Clone)]
pub struct L2StrategicPosition {
    /// 工具使用频率
    pub tool_frequency: HashMap<String, usize>,
    /// 知识主题分布
    pub topic_distribution: HashMap<String, usize>,
    /// 决策数
    pub decision_count: usize,
    /// 架构讨论数
    pub architecture_count: usize,
}

/// L3: 协作效能
#[derive(Debug, Clone)]
pub struct L3CollaborationEfficiency {
    /// 协作模式分布
    pub collaboration_modes: HashMap<String, usize>,
    /// 阻力/摩擦总数
    pub friction_count: usize,
    /// Bug 修复数
    pub bugs_fixed_count: usize,
    /// 代码产出数
    pub code_artifacts_count: usize,
}

/// 从 Observation Items 聚合 L1-L3 报告
pub fn aggregate_observations(items: &[Item]) -> AggregationReport {
    let observations: Vec<&Item> = items
        .iter()
        .filter(|i| i.item_type() == ItemType::Observation)
        .collect();

    let mut l1 = L1CognitiveEvolution {
        cognitive_levels: HashMap::new(),
        total_questions: 0,
        knowledge_gained_count: 0,
    };

    let mut l2 = L2StrategicPosition {
        tool_frequency: HashMap::new(),
        topic_distribution: HashMap::new(),
        decision_count: 0,
        architecture_count: 0,
    };

    let mut l3 = L3CollaborationEfficiency {
        collaboration_modes: HashMap::new(),
        friction_count: 0,
        bugs_fixed_count: 0,
        code_artifacts_count: 0,
    };

    // 计算有多少个独立会话（通过 document_id 去重）
    let session_ids: std::collections::HashSet<_> = observations
        .iter()
        .filter_map(|i| i.document_id().map(|d| d.as_str().to_string()))
        .collect();
    let total_sessions = session_ids.len();

    for item in &observations {
        let content = item.content();
        let tags: Vec<&str> = item.tags().iter().map(|t| t.as_str()).collect();

        // 按标签分类聚合
        if tags.contains(&"decision") {
            l2.decision_count += 1;
        } else if tags.contains(&"bugfix") {
            l3.bugs_fixed_count += 1;
        } else {
            // 综合 observation（含认知水平、协作模式等）
            aggregate_summary_observation(content, &tags, &mut l1, &mut l2, &mut l3);
        }
    }

    AggregationReport {
        total_sessions,
        l1,
        l2,
        l3,
    }
}

fn aggregate_summary_observation(
    content: &str,
    tags: &[&str],
    l1: &mut L1CognitiveEvolution,
    l2: &mut L2StrategicPosition,
    l3: &mut L3CollaborationEfficiency,
) {
    // 从标签提取认知水平和协作模式
    let cognitive_levels = [
        "novice",
        "advanced_beginner",
        "competent",
        "proficient",
        "expert",
    ];
    let collab_modes = [
        "delegation",
        "pair_programming",
        "review",
        "exploration",
        "teaching",
        "deep_inquiry",
    ];

    for tag in tags {
        if cognitive_levels.contains(tag) {
            *l1.cognitive_levels.entry(tag.to_string()).or_insert(0) += 1;
        }
        if collab_modes.contains(tag) {
            *l3.collaboration_modes.entry(tag.to_string()).or_insert(0) += 1;
        }
    }

    // 从内容解析各维度计数
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("- ") {
            let section = find_current_section(content, line);
            match section.as_deref() {
                Some("问题") => l1.total_questions += 1,
                Some("知识") => l1.knowledge_gained_count += 1,
                Some("工具") => {
                    let tool = line.trim_start_matches("- ").to_string();
                    *l2.tool_frequency.entry(tool).or_insert(0) += 1;
                }
                Some("架构") => l2.architecture_count += 1,
                Some("阻力") => l3.friction_count += 1,
                Some("代码产出") => l3.code_artifacts_count += 1,
                _ => {}
            }
        }
    }

    // 从标签构建主题分布
    for tag in tags {
        if !cognitive_levels.contains(tag) && !collab_modes.contains(tag) {
            *l2.topic_distribution.entry(tag.to_string()).or_insert(0) += 1;
        }
    }
}

/// 向上搜索内容中当前行所属的 section 标题
fn find_current_section(content: &str, target_line: &str) -> Option<String> {
    let mut current_section = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.starts_with('-') {
            current_section = Some(trimmed.trim_end_matches(':').to_string());
        }
        if trimmed == target_line {
            return current_section;
        }
    }
    None
}

/// 格式化 L1-L3 报告为可读文本
pub fn format_report(report: &AggregationReport) -> String {
    let mut out = String::new();
    out.push_str("=== Session Insights 报告 ===\n");
    out.push_str(&format!("分析会话数: {}\n\n", report.total_sessions));

    // L1
    out.push_str("── L1: 认知演进 ──\n");
    if !report.l1.cognitive_levels.is_empty() {
        let mut levels: Vec<_> = report.l1.cognitive_levels.iter().collect();
        levels.sort_by(|a, b| b.1.cmp(a.1));
        for (level, count) in &levels {
            out.push_str(&format!("  {}: {}\n", level, count));
        }
    }
    out.push_str(&format!("  提问数: {}\n", report.l1.total_questions));
    out.push_str(&format!(
        "  知识获取: {}\n\n",
        report.l1.knowledge_gained_count
    ));

    // L2
    out.push_str("── L2: 战略定位 ──\n");
    out.push_str(&format!("  决策数: {}\n", report.l2.decision_count));
    out.push_str(&format!("  架构讨论: {}\n", report.l2.architecture_count));
    if !report.l2.topic_distribution.is_empty() {
        let mut topics: Vec<_> = report.l2.topic_distribution.iter().collect();
        topics.sort_by(|a, b| b.1.cmp(a.1));
        out.push_str("  主题分布:\n");
        for (topic, count) in topics.iter().take(10) {
            out.push_str(&format!("    {}: {}\n", topic, count));
        }
    }
    out.push('\n');

    // L3
    out.push_str("── L3: 协作效能 ──\n");
    if !report.l3.collaboration_modes.is_empty() {
        let mut modes: Vec<_> = report.l3.collaboration_modes.iter().collect();
        modes.sort_by(|a, b| b.1.cmp(a.1));
        for (mode, count) in &modes {
            out.push_str(&format!("  {}: {}\n", mode, count));
        }
    }
    out.push_str(&format!("  阻力: {}\n", report.l3.friction_count));
    out.push_str(&format!("  Bug 修复: {}\n", report.l3.bugs_fixed_count));
    out.push_str(&format!("  代码产出: {}\n", report.l3.code_artifacts_count));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{DocumentId, Item, Tag};

    fn make_summary_observation(
        cognitive: &str,
        collab: &str,
        content: &str,
        doc_id: &DocumentId,
    ) -> Item {
        let mut item = Item::new_observation("test", "test");
        item.set_content(content);
        item.set_document_id(doc_id.clone());
        let tags = [Tag::try_new(cognitive), Tag::try_new(collab)]
            .into_iter()
            .flatten()
            .collect();
        let _ = item.set_tags(tags);
        item
    }

    fn make_tagged_observation(tag: &str, doc_id: &DocumentId) -> Item {
        let mut item = Item::new_observation("tagged", "tagged");
        item.set_document_id(doc_id.clone());
        let tags = Tag::try_new(tag).into_iter().collect();
        let _ = item.set_tags(tags);
        item
    }

    #[test]
    fn aggregate_counts_cognitive_levels() {
        let doc1 = DocumentId::new();
        let doc2 = DocumentId::new();
        let items = vec![
            make_summary_observation("proficient", "pair_programming", "", &doc1),
            make_summary_observation("expert", "exploration", "", &doc2),
            make_tagged_observation("decision", &doc1),
            make_tagged_observation("bugfix", &doc2),
        ];

        let report = aggregate_observations(&items);
        assert_eq!(report.total_sessions, 2);
        assert_eq!(report.l1.cognitive_levels.get("proficient"), Some(&1));
        assert_eq!(report.l1.cognitive_levels.get("expert"), Some(&1));
        assert_eq!(report.l2.decision_count, 1);
        assert_eq!(report.l3.bugs_fixed_count, 1);
        assert_eq!(
            report.l3.collaboration_modes.get("pair_programming"),
            Some(&1)
        );
    }

    #[test]
    fn format_report_produces_readable_output() {
        let doc1 = DocumentId::new();
        let items = vec![make_summary_observation(
            "proficient",
            "pair_programming",
            "问题:\n- 如何优化？",
            &doc1,
        )];
        let report = aggregate_observations(&items);
        let text = format_report(&report);
        assert!(text.contains("L1: 认知演进"));
        assert!(text.contains("proficient: 1"));
    }
}
