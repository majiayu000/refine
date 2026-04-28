//! insights v2 — 两级分析：本地聚类 + 10 路 LLM 并发

use anyhow::{Context, Result};
use refine_core::infra::{llm_with_retry_policy, LlmClient, LlmRetryPolicy};
use refine_core::knowledge::{Document, DocumentRepository, ItemRepository, ItemType};
use refine_core::session::{
    build_final_prompt, cluster_observations, merge_route_results, plan_routes, RouteResult,
    INSIGHTS_SYSTEM_PROMPT, ROUTE_SYSTEM_PROMPT,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

const LLM_CONCURRENCY: usize = 10;

#[allow(dead_code)]
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
    let client = match &llm_client {
        Some(c) => c.clone(),
        None => {
            println!("insights 需要 LLM。请配置 API Key。");
            return Ok(());
        }
    };

    // Step 0: 加载全量 observation
    let observations = item_store
        .find_by_type(ItemType::Observation)
        .await
        .context("加载 Observation 失败")?;

    if observations.is_empty() {
        println!("暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。");
        return Ok(());
    }

    println!("加载 {} 条观测数据...", observations.len());

    // Step 1: 本地聚类（纯 Rust，无 LLM）
    let cluster_result = cluster_observations(&observations);
    let stats = &cluster_result.global_stats;

    println!(
        "聚类完成: {} 个项目, {} sessions, {} decisions, {} bugs\n",
        stats.project_ranking.len(),
        stats.total_sessions,
        stats.total_decisions,
        stats.total_bugfixes,
    );

    // Step 2: 规划 LLM 分析路由
    let routes = plan_routes(&cluster_result);
    println!("规划 {} 路并发 LLM 分析...\n", routes.len());

    // Step 3: 并发执行 LLM 分析
    let semaphore = Arc::new(Semaphore::new(LLM_CONCURRENCY));
    let mut handles = Vec::new();

    for route in routes {
        let sem = semaphore.clone();
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let content = match llm_with_retry_policy(
                &client,
                &route.prompt,
                ROUTE_SYSTEM_PROMPT,
                LlmRetryPolicy::default(),
                |attempt, max_retries, delay_secs, _err| {
                    eprintln!(
                        "    ⏳ 重试 ({}/{}) 等待 {}s...",
                        attempt, max_retries, delay_secs
                    );
                },
            )
            .await
            {
                Ok(c) => c,
                Err(e) => format!("分析失败: {}", e),
            };
            eprintln!("  ✓ Route {}: {}", route.id, route.title);
            RouteResult {
                route_id: route.id,
                route_title: route.title,
                content,
            }
        });
        handles.push(handle);
    }

    let mut route_results = Vec::new();
    for handle in handles {
        if let Ok(result) = handle.await {
            route_results.push(result);
        }
    }

    println!(
        "\n{} 路分析完成，合并生成最终报告...\n",
        route_results.len()
    );

    // Step 4: 合并 + 最终报告
    let combined = merge_route_results(&route_results);
    let final_prompt = build_final_prompt(&combined, stats, options.with_prescription);

    let report = llm_with_retry_policy(
        &client,
        &final_prompt,
        INSIGHTS_SYSTEM_PROMPT,
        LlmRetryPolicy::default(),
        |attempt, max_retries, delay_secs, _err| {
            eprintln!(
                "    ⏳ 重试 ({}/{}) 等待 {}s...",
                attempt, max_retries, delay_secs
            );
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("最终报告生成失败: {}", e))?;

    println!("{}", report);

    // Step 5: 保存
    let mut doc = Document::new("session-insights-v2", &report);
    let title = format!(
        "Session Insights v2 {}",
        doc.created_at().format("%Y-%m-%d %H:%M")
    );
    doc.set_title(&title);
    doc.set_url(&format!("insights-v2://{}", doc.created_at().to_rfc3339()));
    doc_store.save(&doc).await.context("保存报告失败")?;

    println!("\n报告已保存 (ID: {})", doc.id());

    Ok(())
}
