//! insights 命令实现
//!
//! 生成 L1-L4 认知洞察报告

use anyhow::{Context, Result};
use refine_core::infra::LlmClient;
use refine_core::knowledge::{ItemRepository, ItemType};
use refine_core::session::{
    aggregate_observations, build_prescription_prompt, format_report, PRESCRIPTION_SYSTEM_PROMPT,
};
use std::sync::Arc;

pub struct InsightsOptions {
    pub period: Option<usize>,
    pub with_prescription: bool,
}

pub async fn handle_insights(
    options: InsightsOptions,
    item_store: Arc<dyn ItemRepository>,
    llm_client: Option<Arc<dyn LlmClient>>,
) -> Result<()> {
    // 1. 加载所有 Observation Items
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .context("加载 Observation 失败")?;

    if observations.is_empty() {
        println!("暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。");
        return Ok(());
    }

    // TODO: period 过滤（按 created_at 筛选最近 N 天的 items）
    let items_to_analyze = if let Some(_period) = options.period {
        // 暂时使用全量，后续按时间过滤
        observations
    } else {
        observations
    };

    println!("分析 {} 条观测数据...\n", items_to_analyze.len());

    // 2. L1-L3 聚合
    let report = aggregate_observations(&items_to_analyze);
    let report_text = format_report(&report);
    println!("{}", report_text);

    // 3. L4 处方（需要 LLM）
    if options.with_prescription {
        let Some(client) = llm_client else {
            println!("未配置 LLM，跳过 L4 处方生成。");
            return Ok(());
        };

        println!("\n生成 L4 处方...\n");
        let prompt = build_prescription_prompt(&report);
        let response = client
            .complete(&prompt, Some(PRESCRIPTION_SYSTEM_PROMPT))
            .await
            .map_err(|e| anyhow::anyhow!("LLM 调用失败: {}", e))?;

        println!("── L4: 成长处方 ──\n");
        println!("{}", response);
    }

    Ok(())
}
