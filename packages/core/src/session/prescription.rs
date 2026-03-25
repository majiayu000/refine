//! L4 处方生成
//!
//! 基于 L1-L3 聚合结果生成个性化提升建议

use super::aggregation::AggregationReport;

/// 处方系统 prompt
pub const PRESCRIPTION_SYSTEM_PROMPT: &str =
    "你是技术成长教练。基于开发者的编程会话分析数据，生成个性化提升建议。使用中文回答。";

/// 构建处方生成 prompt
pub fn build_prescription_prompt(report: &AggregationReport) -> String {
    let mut context = String::new();

    context.push_str(&format!("分析会话数: {}\n\n", report.total_sessions));

    // L1 数据
    context.push_str("认知水平分布:\n");
    let mut levels: Vec<_> = report.l1.cognitive_levels.iter().collect();
    levels.sort_by(|a, b| b.1.cmp(a.1));
    for (level, count) in &levels {
        context.push_str(&format!("  {}: {}\n", level, count));
    }
    context.push_str(&format!("提问数: {}\n", report.l1.total_questions));
    context.push_str(&format!(
        "知识获取数: {}\n\n",
        report.l1.knowledge_gained_count
    ));

    // L2 数据
    context.push_str(&format!("决策数: {}\n", report.l2.decision_count));
    context.push_str(&format!("架构讨论数: {}\n", report.l2.architecture_count));
    if !report.l2.topic_distribution.is_empty() {
        context.push_str("主题分布:\n");
        let mut topics: Vec<_> = report.l2.topic_distribution.iter().collect();
        topics.sort_by(|a, b| b.1.cmp(a.1));
        for (topic, count) in topics.iter().take(15) {
            context.push_str(&format!("  {}: {}\n", topic, count));
        }
    }
    if !report.l2.tool_frequency.is_empty() {
        context.push_str("工具使用:\n");
        let mut tools: Vec<_> = report.l2.tool_frequency.iter().collect();
        tools.sort_by(|a, b| b.1.cmp(a.1));
        for (tool, count) in tools.iter().take(10) {
            context.push_str(&format!("  {}: {}\n", tool, count));
        }
    }
    context.push('\n');

    // L3 数据
    context.push_str("协作模式:\n");
    let mut modes: Vec<_> = report.l3.collaboration_modes.iter().collect();
    modes.sort_by(|a, b| b.1.cmp(a.1));
    for (mode, count) in &modes {
        context.push_str(&format!("  {}: {}\n", mode, count));
    }
    context.push_str(&format!("阻力数: {}\n", report.l3.friction_count));
    context.push_str(&format!("Bug 修复数: {}\n", report.l3.bugs_fixed_count));
    context.push_str(&format!("代码产出数: {}\n", report.l3.code_artifacts_count));

    format!(
        r#"基于以下开发者编程会话的聚合分析数据，生成个性化技术成长处方。

数据:
{context}

请生成包含以下部分的处方:

1. **技能路线图**: 基于认知水平和主题分布，推荐下一步应深入的 3-5 个技术方向
2. **学习策略**: 基于提问模式和知识获取情况，优化学习方法的建议
3. **时间分配**: 基于协作模式分布，建议探索/利用的时间比例调整
4. **AI 协作调优**: 基于协作模式，建议如何更有效地与 AI 工具协作
5. **季度 OKR**: 基于以上分析，制定 3 个可量化的季度目标

每部分用简洁的要点列出，重点突出可操作的具体建议。"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn build_prescription_prompt_includes_all_sections() {
        let report = AggregationReport {
            total_sessions: 10,
            l1: super::super::aggregation::L1CognitiveEvolution {
                cognitive_levels: HashMap::from([("proficient".to_string(), 7)]),
                total_questions: 15,
                knowledge_gained_count: 20,
            },
            l2: super::super::aggregation::L2StrategicPosition {
                tool_frequency: HashMap::from([("cargo".to_string(), 5)]),
                topic_distribution: HashMap::from([("rust".to_string(), 8)]),
                decision_count: 12,
                architecture_count: 3,
            },
            l3: super::super::aggregation::L3CollaborationEfficiency {
                collaboration_modes: HashMap::from([("pair_programming".to_string(), 6)]),
                friction_count: 4,
                bugs_fixed_count: 8,
                code_artifacts_count: 15,
            },
        };

        let prompt = build_prescription_prompt(&report);
        assert!(prompt.contains("技能路线图"));
        assert!(prompt.contains("proficient: 7"));
        assert!(prompt.contains("rust: 8"));
        assert!(prompt.contains("pair_programming: 6"));
    }
}
