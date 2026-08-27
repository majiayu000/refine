//! 10 路 LLM 分析任务定义
//!
//! 每路专注一个维度，构建 ≤25K 的压缩上下文

use super::clustering::{ClusterResult, ProjectCluster};

const MAX_ROUTE_CONTEXT: usize = 25_000;

/// 分析路由
#[derive(Debug, Clone)]
pub struct AnalysisRoute {
    pub id: usize,
    pub title: String,
    pub prompt: String,
}

/// 根据聚类结果规划 10 路分析任务
pub fn plan_routes(cluster: &ClusterResult) -> Vec<AnalysisRoute> {
    let mut routes = Vec::new();
    let stats = &cluster.global_stats;

    routes.push(build_project_overview(1, cluster));
    routes.push(build_decision_patterns(2, cluster));
    routes.push(build_bug_patterns(3, cluster));
    routes.push(build_cognitive_evolution(4, cluster));
    routes.push(build_tech_radar(5, cluster));
    routes.push(build_ai_collaboration(6, cluster));
    routes.push(build_workflow_patterns(7, cluster));

    // Route 8-10: 大项目专项分析
    let big_projects: Vec<&ProjectCluster> = stats
        .project_ranking
        .iter()
        .filter_map(|(name, count)| {
            if *count >= 15 {
                cluster.projects.get(name)
            } else {
                None
            }
        })
        .take(3)
        .collect();

    for (i, project) in big_projects.iter().enumerate() {
        routes.push(build_project_deep_dive(8 + i, project));
    }

    // 补足到 10 路
    if routes.len() < 10 {
        routes.push(build_knowledge_network(routes.len() + 1, cluster));
    }
    if routes.len() < 10 {
        routes.push(build_friction_deep_dive(routes.len() + 1, cluster));
    }

    let quality = &cluster.data_quality;
    let trend_guard = if quality.is_degraded() {
        "数据质量为 DEGRADED；脱链观测已排除。禁止输出跨期趋势、增减或改善/退化判断。"
    } else {
        "这是单一 cohort 聚合；没有显式时序证据时，不得推断趋势。"
    };
    let cohort_header = format!(
        "## Cohort contract\neligible linked observations: {} | detached excluded: {} | mode excluded: {} | linked ratio: {:.1}%\n{}\n\n",
        quality.eligible_observations,
        quality.detached_observations,
        quality.mode_excluded_observations,
        quality.linked_ratio() * 100.0,
        trend_guard,
    );
    for route in &mut routes {
        route.prompt.insert_str(0, &cohort_header);
    }

    routes
}

fn build_project_overview(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let stats = &cluster.global_stats;
    let mut ctx = format!(
        "总会话: {}, 总决策: {}, 总Bug修复: {}, 项目数: {}\n\n",
        stats.total_sessions,
        stats.total_decisions,
        stats.total_bugfixes,
        stats.project_ranking.len()
    );

    for (name, count) in stats.project_ranking.iter().take(15) {
        if let Some(p) = cluster.projects.get(name) {
            ctx.push_str(&format!(
                "## {} ({} sessions, {} decisions, {} bugs)\n",
                name,
                count,
                p.decision_titles.len(),
                p.bugfix_titles.len()
            ));
            for excerpt in p.summary_excerpts.iter().take(3) {
                ctx.push_str(&format!("{}\n", excerpt));
            }
            ctx.push('\n');
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "项目全景".to_string(),
        prompt: format!(
            r#"## 分析任务: 项目全景

按项目/领域分组，描述每个项目具体做了什么。
对每个重要项目（session >= 5），写 3-5 句叙事描述:
- 项目是什么、解决什么问题
- 做了哪些关键工作
- 代表性的技术决策或成就
小项目合并为综述。用"你在 xx 中做了 xx"的第二人称。

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_decision_patterns(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let mut ctx = String::new();
    for (name, _) in cluster.global_stats.project_ranking.iter().take(12) {
        if let Some(p) = cluster.projects.get(name) {
            if p.decision_titles.is_empty() {
                continue;
            }
            ctx.push_str(&format!(
                "## {} ({} decisions)\n",
                name,
                p.decision_titles.len()
            ));
            for d in p.decision_titles.iter().take(25) {
                ctx.push_str(&format!("- {}\n", d));
            }
            ctx.push('\n');
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "技术决策模式".to_string(),
        prompt: format!(
            r#"## 分析任务: 技术决策模式

从决策列表中:
1. 归纳 3-5 个反复出现的技术选型模式（如"偏好类型安全"、"选择嵌入式DB"）
2. 识别架构偏好（如"单体优先"、"trait抽象"）
3. 找出值得注意的不寻常决策
4. 每个模式引用 3-5 个具体决策作为证据

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_bug_patterns(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let mut ctx = String::new();
    for (name, _) in cluster.global_stats.project_ranking.iter().take(12) {
        if let Some(p) = cluster.projects.get(name) {
            if p.bugfix_titles.is_empty() && p.frictions.is_empty() {
                continue;
            }
            ctx.push_str(&format!(
                "## {} ({} bugs, {} frictions)\n",
                name,
                p.bugfix_titles.len(),
                p.frictions.len()
            ));
            for b in p.bugfix_titles.iter().take(20) {
                ctx.push_str(&format!("- [bug] {}\n", b));
            }
            for f in p.frictions.iter().take(10) {
                ctx.push_str(&format!("- [friction] {}\n", f));
            }
            ctx.push('\n');
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "Bug 与阻力模式".to_string(),
        prompt: format!(
            r#"## 分析任务: Bug 与阻力模式

从 bug 和阻力列表中:
1. 归纳 top 5 最常见 bug 类型（如"类型不匹配"、"并发问题"）
2. 识别反复出现的阻力模式
3. 给出根因分析
4. 每种模式引用 3+ 具体例子
5. 建议可复制到 CLAUDE.md 的预防规则

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_cognitive_evolution(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let stats = &cluster.global_stats;
    let mut ctx = "全局认知水平分布:\n".to_string();
    let mut levels: Vec<_> = stats.cognitive_levels.iter().collect();
    levels.sort_by(|a, b| b.1.cmp(a.1));
    for (level, count) in &levels {
        ctx.push_str(&format!("  {}: {}\n", level, count));
    }

    ctx.push_str("\n按项目认知水平:\n");
    for (name, _) in stats.project_ranking.iter().take(10) {
        if let Some(p) = cluster.projects.get(name) {
            if p.cognitive_levels.is_empty() {
                continue;
            }
            ctx.push_str(&format!("  {}: {:?}\n", name, p.cognitive_levels));
        }
    }

    ctx.push_str("\n知识获取样本:\n");
    for (name, _) in stats.project_ranking.iter().take(10) {
        if let Some(p) = cluster.projects.get(name) {
            for k in p.knowledge_gained.iter().take(5) {
                ctx.push_str(&format!("- [{}] {}\n", name, k));
            }
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "认知演进".to_string(),
        prompt: format!(
            r#"## 分析任务: 认知演进 (Dreyfus/Bloom)

基于认知水平分布和知识获取数据:
1. 评估当前 Dreyfus 阶段（按领域分别评估）
2. Bloom 认知层级: 活动集中在哪个层（应用/分析/评估/创造）
3. 双环学习: 是否有质疑底层假设的迹象
4. 学习曲线: 知识获取主题是否在变深
5. 各项目的认知水平差异说明什么

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_tech_radar(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let mut ctx = String::new();
    let mut tools: Vec<_> = cluster.global_stats.tool_frequency.iter().collect();
    tools.sort_by(|a, b| b.1.cmp(a.1));
    ctx.push_str("工具使用频率:\n");
    for (tool, count) in tools.iter().take(50) {
        ctx.push_str(&format!("  {}: {}\n", tool, count));
    }

    ctx.push_str("\n架构讨论:\n");
    for (name, _) in cluster.global_stats.project_ranking.iter().take(10) {
        if let Some(p) = cluster.projects.get(name) {
            for a in p.architectures.iter().take(5) {
                ctx.push_str(&format!("- [{}] {}\n", name, a));
            }
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "技术雷达".to_string(),
        prompt: format!(
            r#"## 分析任务: 个人技术雷达

基于工具使用频率和架构讨论:
1. 构建个人技术雷达，分类为:
   - Adopt: 高频使用且跨多项目
   - Trial: 近期开始使用
   - Assess: 讨论过但未广泛使用
   - Hold: 频率在下降
2. 用 markdown 表格展示
3. 分析技术栈的广度和深度

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_ai_collaboration(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let stats = &cluster.global_stats;
    let mut ctx = "全局协作模式分布:\n".to_string();
    let mut modes: Vec<_> = stats.collaboration_modes.iter().collect();
    modes.sort_by(|a, b| b.1.cmp(a.1));
    for (mode, count) in &modes {
        ctx.push_str(&format!("  {}: {}\n", mode, count));
    }

    ctx.push_str("\nAI 相关摩擦:\n");
    for (name, _) in stats.project_ranking.iter().take(10) {
        if let Some(p) = cluster.projects.get(name) {
            for f in p.frictions.iter().take(8) {
                ctx.push_str(&format!("- [{}] {}\n", name, f));
            }
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "AI 协作效能".to_string(),
        prompt: format!(
            r#"## 分析任务: AI 协作效能

基于协作模式和摩擦数据:
1. 协作模式光谱: 各模式占比和适用性
2. AI 导致的摩擦类型: 方向错误、代码质量、理解偏差等
3. 有效协作的模式特征
4. 给出 3-5 条可操作的 AI 协作优化建议

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_workflow_patterns(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let stats = &cluster.global_stats;
    let mut ctx = "项目活跃度排名:\n".to_string();
    for (name, count) in stats.project_ranking.iter().take(20) {
        ctx.push_str(&format!("  {}: {} sessions\n", name, count));
    }
    ctx.push_str(&format!("\n总项目数: {}\n", stats.project_ranking.len()));

    let big = stats
        .project_ranking
        .iter()
        .filter(|(_, c)| *c >= 10)
        .count();
    let medium = stats
        .project_ranking
        .iter()
        .filter(|(_, c)| *c >= 3 && *c < 10)
        .count();
    let small = stats.project_ranking.iter().filter(|(_, c)| *c < 3).count();
    ctx.push_str(&format!(
        "大项目(>=10): {}, 中项目(3-9): {}, 小项目(<3): {}\n",
        big, medium, small
    ));

    AnalysisRoute {
        id,
        title: "工作流与时间模式".to_string(),
        prompt: format!(
            r#"## 分析任务: 工作流与注意力分配

基于项目分布数据:
1. 注意力分配: 时间主要投在哪些项目
2. 探索/利用比: 新项目 vs 持续深耕的比例（15-30% 探索为健康）
3. 碎片化程度: 是否在过多项目间切换
4. 深度工作: 哪些项目有持续深入

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_project_deep_dive(id: usize, project: &ProjectCluster) -> AnalysisRoute {
    let mut ctx = format!(
        "项目: {} ({} sessions, {} decisions, {} bugs)\n\n",
        project.project_name,
        project.session_count,
        project.decision_titles.len(),
        project.bugfix_titles.len()
    );

    ctx.push_str("会话摘要:\n");
    for excerpt in project.summary_excerpts.iter().take(10) {
        ctx.push_str(&format!("{}\n\n", excerpt));
    }

    ctx.push_str("关键决策:\n");
    for d in project.decision_titles.iter().take(30) {
        ctx.push_str(&format!("- {}\n", d));
    }

    ctx.push_str("\nBug 修复:\n");
    for b in project.bugfix_titles.iter().take(20) {
        ctx.push_str(&format!("- {}\n", b));
    }

    if !project.patterns.is_empty() {
        ctx.push_str("\n模式:\n");
        for p in project.patterns.iter().take(10) {
            ctx.push_str(&format!("- {}\n", p));
        }
    }

    AnalysisRoute {
        id,
        title: format!("{} 专项分析", project.project_name),
        prompt: format!(
            r#"## 分析任务: {} 项目深度分析

对这个项目做详细技术叙事:
1. 项目目标和解决的问题
2. 关键技术成就
3. 架构和设计选择
4. 主要痛点和 bug 模式
5. 改进建议

## 数据
{}"#,
            project.project_name,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_knowledge_network(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let mut ctx = String::new();
    for (name, _) in cluster.global_stats.project_ranking.iter().take(15) {
        if let Some(p) = cluster.projects.get(name) {
            if p.knowledge_gained.is_empty() {
                continue;
            }
            ctx.push_str(&format!("## {} 知识获取\n", name));
            for k in p.knowledge_gained.iter().take(10) {
                ctx.push_str(&format!("- {}\n", k));
            }
            ctx.push('\n');
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "知识网络".to_string(),
        prompt: format!(
            r#"## 分析任务: 知识网络与学习主题

从知识获取数据中:
1. 识别主要知识领域和主题聚类
2. 跨项目的知识复用模式
3. 知识盲区（哪些领域有活动但知识获取少）
4. 学习深度评估

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn build_friction_deep_dive(id: usize, cluster: &ClusterResult) -> AnalysisRoute {
    let mut ctx = String::new();
    for (name, _) in cluster.global_stats.project_ranking.iter().take(15) {
        if let Some(p) = cluster.projects.get(name) {
            if p.frictions.is_empty() {
                continue;
            }
            ctx.push_str(&format!("## {} 阻力\n", name));
            for f in &p.frictions {
                ctx.push_str(&format!("- {}\n", f));
            }
            ctx.push('\n');
        }
        if ctx.chars().count() > MAX_ROUTE_CONTEXT {
            break;
        }
    }

    AnalysisRoute {
        id,
        title: "摩擦深度分析".to_string(),
        prompt: format!(
            r#"## 分析任务: 摩擦与阻力深度分析

从所有项目的阻力数据中:
1. 按类型分类: 环境问题、AI 误判、配置错误、架构不清等
2. 反复出现的模式（跨项目的共性摩擦）
3. 可预防 vs 不可避免的摩擦比例
4. 给出 5 条可操作的预防建议

## 数据
{}"#,
            truncate(&ctx, MAX_ROUTE_CONTEXT)
        ),
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let original_chars = text.chars().count();
    if original_chars <= max_chars {
        return text.to_string();
    }
    let mut result = String::new();
    let mut used_chars = 0usize;
    for line in text.lines() {
        let line_chars = line.chars().count();
        if used_chars + line_chars + 1 > max_chars {
            result.push_str(&format!("\n... (截断，原始 {} 字符)", original_chars));
            break;
        }
        result.push_str(line);
        result.push('\n');
        used_chars += line_chars + 1;
    }
    result
}

#[cfg(test)]
mod unicode_budget_tests {
    use super::truncate;

    #[test]
    fn truncate_counts_unicode_characters_not_utf8_bytes() {
        let text = "中文一行\n中文二行\n中文三行";
        let truncated = truncate(text, 12);
        assert!(truncated.contains("中文一行"));
        assert!(truncated.contains("中文二行"));
    }
}
