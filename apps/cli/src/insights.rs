//! insights 命令实现
//!
//! 把 observation 的具体内容直接喂给 LLM，生成有实质的报告

use anyhow::{Context, Result};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{Document, DocumentRepository, Item, ItemRepository, ItemType};
use std::sync::Arc;

const INSIGHTS_SYSTEM_PROMPT: &str = "你是技术成长分析师。基于开发者的真实编程会话观测数据，生成具体、有实质内容的洞察报告。\
所有判断必须引用具体的会话数据作为证据。禁止泛泛而谈。使用中文。";

pub struct InsightsOptions {
    pub period: Option<usize>,
    pub with_prescription: bool,
}

pub async fn handle_insights(
    options: InsightsOptions,
    item_store: Arc<dyn ItemRepository>,
    doc_store: Arc<dyn DocumentRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .context("加载 Observation 失败")?;

    if observations.is_empty() {
        println!("暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。");
        return Ok(());
    }

    let items = if let Some(_period) = options.period {
        observations
    } else {
        observations
    };

    println!("加载 {} 条观测数据，构建分析上下文...\n", items.len());

    let client = match &llm_client {
        Some(c) => c.as_ref(),
        None => {
            println!("insights 需要 LLM。请配置 API Key。");
            return Ok(());
        }
    };

    // 把具体内容整理成结构化文本喂给 LLM
    let context = build_rich_context(&items);

    println!("上下文: {} 字符，发送给 LLM...\n", context.len());

    let prompt = build_insights_prompt(&context, options.with_prescription);
    let response = client
        .complete(&prompt, Some(INSIGHTS_SYSTEM_PROMPT))
        .await
        .map_err(|e| anyhow::anyhow!("LLM 调用失败: {}", e))?;

    println!("{}", response);

    // 保存到数据库
    let mut doc = Document::new("session-insights", &response);
    let title = format!(
        "Session Insights {}",
        doc.created_at().format("%Y-%m-%d %H:%M")
    );
    doc.set_title(&title);
    doc.set_url(&format!("insights://{}", doc.created_at().to_rfc3339()));
    doc_store.save(&doc).await.context("保存报告失败")?;

    println!("\n报告已保存 (ID: {})", doc.id());
    println!("查看: refine doc-show {}", doc.id());

    Ok(())
}

/// 从 observations 中提取具体内容，按类别分组
fn build_rich_context(items: &[Item]) -> String {
    let mut summaries = Vec::new();
    let mut decisions = Vec::new();
    let mut bugs = Vec::new();
    let mut other_observations = Vec::new();

    for item in items {
        let tags: Vec<&str> = item.tags().iter().map(|t| t.as_str()).collect();
        let title = item.title();
        let content = item.content();

        if tags.contains(&"decision") {
            decisions.push(title.to_string());
        } else if tags.contains(&"bugfix") {
            bugs.push(title.to_string());
        } else {
            // 综合 observation — 包含会话摘要和完整内容
            summaries.push(format!("【{}】\n{}", title, content));
            // 从内容中提取有标签的其他条目
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("- ") && line.len() > 4 {
                    other_observations.push(line.trim_start_matches("- ").to_string());
                }
            }
        }
    }

    let mut out = String::new();

    const MAX_CONTEXT_CHARS: usize = 30_000;

    // 会话摘要
    out.push_str(&format!("=== 会话观测详情 ({} 个会话) ===\n\n", summaries.len()));
    for summary in &summaries {
        if out.len() + summary.len() > MAX_CONTEXT_CHARS / 2 {
            break;
        }
        out.push_str(summary);
        out.push_str("\n\n---\n\n");
    }

    // 具体决策列表
    if !decisions.is_empty() {
        out.push_str(&format!("\n=== 技术决策 ({} 条) ===\n", decisions.len()));
        for (i, d) in decisions.iter().enumerate() {
            if out.len() > MAX_CONTEXT_CHARS * 3 / 4 {
                out.push_str(&format!("... 还有 {} 条\n", decisions.len() - i));
                break;
            }
            out.push_str(&format!("{}. {}\n", i + 1, d));
        }
    }

    // 具体 bug 列表
    if !bugs.is_empty() {
        out.push_str(&format!("\n=== Bug 修复 ({} 条) ===\n", bugs.len()));
        for (i, b) in bugs.iter().enumerate() {
            if out.len() > MAX_CONTEXT_CHARS {
                out.push_str(&format!("... 还有 {} 条\n", bugs.len() - i));
                break;
            }
            out.push_str(&format!("{}. {}\n", i + 1, b));
        }
    }

    out
}

fn build_insights_prompt(context: &str, with_prescription: bool) -> String {
    let prescription_section = if with_prescription {
        r#"

## L4: 成长处方
基于以上所有具体数据：
- **你做得好的地方**：引用具体的会话和决策
- **阻力热点**：列出反复出现的具体问题模式
- **技能路线图**：基于你实际做的项目推荐 3-5 个深入方向
- **AI 协作改进**：基于协作模式的具体观察给建议
- **下一步行动**：3 个可立即执行的具体行动"#
    } else {
        ""
    };

    format!(
        r#"以下是一位开发者最近编程会话的完整观测数据。请基于这些**具体内容**生成洞察报告。

要求：
- 每个判断必须引用具体的会话/决策/bug 作为证据
- 禁止输出 "competent: 32" 这样的纯计数
- 用具体项目名、工具名、技术栈代替泛化描述
- 直接说"你在 xx 项目中做了 xx"，不要说"该开发者倾向于..."

{context}

---

请生成以下报告：

## L1: 你在做什么
按项目/领域分组，描述每个领域具体做了什么，列出关键成就。

## L2: 技术决策与模式
从决策列表中提炼关键的技术选型和架构模式，指出反复出现的决策倾向。

## L3: 阻力与 Bug 模式
从 bug 列表和阻力描述中归纳具体的问题类型，指出根因模式。{prescription_section}"#
    )
}
