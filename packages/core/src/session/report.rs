//! 报告合并与最终 prompt 生成
//!
//! 将 10 路 LLM 分析结果合并为最终报告

use super::clustering::GlobalStats;

const MAX_FINAL_CONTEXT: usize = 30_000;

/// 单路分析结果
#[derive(Debug, Clone)]
pub struct RouteResult {
    pub route_id: usize,
    pub route_title: String,
    pub content: String,
}

/// 合并所有路由结果为结构化文本
pub fn merge_route_results(results: &[RouteResult]) -> String {
    let mut out = String::new();
    let mut sorted = results.to_vec();
    sorted.sort_by_key(|r| r.route_id);

    for r in &sorted {
        out.push_str(&format!("## {}\n\n", r.route_title));
        out.push_str(&r.content);
        out.push_str("\n\n---\n\n");
    }
    out
}

/// 格式化全局统计摘要
pub fn format_global_stats(stats: &GlobalStats) -> String {
    let mut out = format!(
        "总会话: {} | 总决策: {} | 总Bug修复: {} | 总结构化观测: {} | 项目数: {}\n",
        stats.total_sessions,
        stats.total_decisions,
        stats.total_bugfixes,
        stats.total_summaries,
        stats.project_ranking.len(),
    );

    out.push_str("\n认知水平分布: ");
    let mut levels: Vec<_> = stats.cognitive_levels.iter().collect();
    levels.sort_by(|a, b| b.1.cmp(a.1));
    let level_strs: Vec<String> = levels.iter().map(|(k, v)| format!("{}({})", k, v)).collect();
    out.push_str(&level_strs.join(", "));

    out.push_str("\n协作模式分布: ");
    let mut modes: Vec<_> = stats.collaboration_modes.iter().collect();
    modes.sort_by(|a, b| b.1.cmp(a.1));
    let mode_strs: Vec<String> = modes.iter().map(|(k, v)| format!("{}({})", k, v)).collect();
    out.push_str(&mode_strs.join(", "));

    out.push_str("\n\nTop 10 项目: ");
    let top: Vec<String> = stats
        .project_ranking
        .iter()
        .take(10)
        .map(|(name, count)| format!("{}({})", name, count))
        .collect();
    out.push_str(&top.join(", "));

    out
}

/// 构建最终报告的 LLM prompt
pub fn build_final_prompt(
    combined_analysis: &str,
    stats: &GlobalStats,
    with_prescription: bool,
) -> String {
    let stats_summary = format_global_stats(stats);

    let prescription_section = if with_prescription {
        r#"

## 成长处方
基于以上所有分析:
- **技能路线图**: 深耕/突破/放弃/盲区四象限
- **3 个可立即执行的行动**（具体到文件或命令级别）
- **季度 OKR**（3 个目标，每个 3-5 个可量化 KR）
- **可复制到 CLAUDE.md 的具体规则**（至少 5 条）"#
    } else {
        ""
    };

    // 截断 combined_analysis 确保不超限
    let max_analysis = MAX_FINAL_CONTEXT - stats_summary.len() - 2000;
    let analysis = if combined_analysis.len() > max_analysis {
        let truncated: String = combined_analysis.chars().take(max_analysis).collect();
        format!("{}\n\n... (部分分析因篇幅截断)", truncated)
    } else {
        combined_analysis.to_string()
    };

    format!(
        r#"以下是对一位开发者 {sessions} 个 AI 编程会话的多维度分析结果。
请综合所有分析，生成一份完整的洞察报告。

## 全局数据
{stats}

## 各维度分析
{analysis}

---

请生成最终报告，严格按以下结构输出 markdown:

# Session Insights Report

## 概览
一段话概括整体状态，附关键数字。引用具体项目名。

## 你在做什么（按项目）
对每个重要项目写一段叙事。用第二人称"你在 xx 中做了 xx"。

## 技术选型与架构偏好
关键的技术决策倾向和架构模式，引用具体决策。

## 阻力热点与改进方向
反复出现的问题，用表格展示: | 问题类型 | 频率 | 典型例子 | 预防建议 |

## 认知状态
Dreyfus 阶段评估（按领域）、学习深度、成长趋势。

## 技术雷达
| 环 | 技术/工具 | 说明 |
用表格展示 Adopt/Trial/Assess/Hold。

## AI 协作效能
当前模式分析和具体优化建议。{prescription_section}"#,
        sessions = stats.total_sessions,
        stats = stats_summary,
        analysis = analysis,
    )
}

pub const INSIGHTS_SYSTEM_PROMPT: &str = "你是技术成长分析师。\
基于开发者的真实编程会话观测数据，生成具体、有实质内容的洞察报告。\
所有判断必须引用具体数据作为证据。禁止泛泛而谈。\
使用中文。用第二人称'你'。";

pub const ROUTE_SYSTEM_PROMPT: &str = "你是技术成长分析师。\
基于开发者的编程会话数据分析特定维度。\
所有判断必须引用具体数据。控制在 3000 字以内。使用中文。";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_route_results_orders_by_id() {
        let results = vec![
            RouteResult { route_id: 3, route_title: "C".into(), content: "c".into() },
            RouteResult { route_id: 1, route_title: "A".into(), content: "a".into() },
        ];
        let merged = merge_route_results(&results);
        assert!(merged.find("## A").unwrap() < merged.find("## C").unwrap());
    }
}
