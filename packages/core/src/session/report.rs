//! 报告合并与最终 prompt 生成
//!
//! 将 10 路 LLM 分析结果合并为最终报告

use super::clustering::{DataQualityStats, GlobalStats};
use serde::{Deserialize, Serialize};

const MAX_FINAL_CONTEXT: usize = 30_000;

/// 单路分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    let level_strs: Vec<String> = levels
        .iter()
        .map(|(k, v)| format!("{}({})", k, v))
        .collect();
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

/// Visible cohort metadata shared by prompts and persisted reports.
pub fn format_data_quality_stats(quality: &DataQualityStats) -> String {
    format!(
        "状态: {} | 输入观测: {} | 已关联: {} ({:.1}%) | 脱链排除: {} | 模式排除: {} | 合格 cohort: {}",
        quality.status_label(),
        quality.input_observations,
        quality.linked_observations,
        quality.linked_ratio() * 100.0,
        quality.detached_observations,
        quality.mode_excluded_observations,
        quality.eligible_observations,
    )
}

/// 构建最终报告的 LLM prompt
pub fn build_final_prompt(
    combined_analysis: &str,
    stats: &GlobalStats,
    quality: &DataQualityStats,
    with_prescription: bool,
) -> String {
    let stats_summary = format_global_stats(stats);
    let quality_summary = format_data_quality_stats(quality);
    let trend_guard = if quality.is_degraded() {
        "数据质量为 DEGRADED。脱链观测已从全部统计和证据中排除；不得据此输出跨期趋势、增减或改善/退化结论。"
    } else {
        "当前输入是单一窗口聚合；只有各维度分析提供显式时序证据时，才可输出趋势结论。"
    };

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

## Cohort 与数据质量
{quality}
{trend_guard}

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
Dreyfus 阶段评估（按领域）、学习深度。仅当有显式时序证据且数据质量允许时写成长趋势，否则明确写“趋势不可判定”。

## 技术雷达
| 环 | 技术/工具 | 说明 |
用表格展示 Adopt/Trial/Assess/Hold。

## AI 协作效能
当前模式分析和具体优化建议。{prescription_section}"#,
        sessions = stats.total_sessions,
        stats = stats_summary,
        quality = quality_summary,
        trend_guard = trend_guard,
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
            RouteResult {
                route_id: 3,
                route_title: "C".into(),
                content: "c".into(),
            },
            RouteResult {
                route_id: 1,
                route_title: "A".into(),
                content: "a".into(),
            },
        ];
        let merged = merge_route_results(&results);
        assert!(merged.find("## A").unwrap() < merged.find("## C").unwrap());
    }

    #[test]
    fn degraded_final_prompt_forbids_trend_claims() {
        let stats = GlobalStats {
            total_sessions: 1,
            total_decisions: 1,
            total_bugfixes: 0,
            total_summaries: 1,
            cognitive_levels: Default::default(),
            collaboration_modes: Default::default(),
            tool_frequency: Default::default(),
            project_ranking: vec![("refine".into(), 1)],
        };
        let quality = DataQualityStats {
            input_observations: 3,
            linked_observations: 2,
            detached_observations: 1,
            mode_excluded_observations: 0,
            eligible_observations: 2,
            cohort_identity: "sha256:test".into(),
        };

        let prompt = build_final_prompt("analysis", &stats, &quality, false);
        assert!(prompt.contains("DEGRADED"));
        assert!(prompt.contains("不得据此输出跨期趋势"));
        assert!(prompt.contains("趋势不可判定"));
    }
}
