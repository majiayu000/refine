//! insights v2 — 两级分析：本地聚类 + 10 路 LLM 并发

use crate::insights_checkpoint::{DatasetSignature, InsightsCheckpoint, CHECKPOINT_VERSION};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use refine_core::infra::{llm_with_retry_policy, LlmClient, LlmRetryPolicy};
use refine_core::knowledge::{Document, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_final_prompt, cluster_observations, merge_route_results, plan_routes, RouteResult,
    INSIGHTS_SYSTEM_PROMPT, ROUTE_SYSTEM_PROMPT,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

const LLM_CONCURRENCY: usize = 10;
const FINAL_REPORT_REQUEST_TIMEOUT_MILLIS: u64 = 300_000;
const INSIGHTS_PROMPT_IDENTITY: &str = "insights-v2:route-v1:final-v1";

fn final_report_retry_policy() -> LlmRetryPolicy {
    LlmRetryPolicy {
        request_timeout_millis: FINAL_REPORT_REQUEST_TIMEOUT_MILLIS,
        ..LlmRetryPolicy::default()
    }
}

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
    if matches!(options.period, Some(0)) {
        anyhow::bail!("--period 必须大于 0");
    }
    let client = match &llm_client {
        Some(c) => c.clone(),
        None => {
            println!("insights 需要 LLM。请配置 API Key。");
            return Ok(());
        }
    };

    let observations = match options.period {
        Some(days) => {
            let days = i64::try_from(days).context("--period 超出支持范围")?;
            let now = Utc::now();
            item_store
                .find_observations_by_event_range(now - Duration::days(days), now)
                .await
        }
        None => {
            item_store
                .find_by_type(refine_core::knowledge::ItemType::Observation)
                .await
        }
    }
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

    let latest_updated_at = observations
        .iter()
        .map(|item| item.updated_at())
        .max()
        .context("Observation 集合缺少更新时间")?;
    let signature = DatasetSignature {
        checkpoint_version: CHECKPOINT_VERSION,
        observation_count: observations.len(),
        latest_updated_at,
        with_prescription: options.with_prescription,
        period_days: options.period,
        llm_identity: client.cache_identity(),
        prompt_identity: INSIGHTS_PROMPT_IDENTITY.to_string(),
    };
    let mut checkpoint = InsightsCheckpoint::load_matching(signature)?;

    // Step 2: 规划 LLM 分析路由
    let routes = plan_routes(&cluster_result);
    let route_count = routes.len();
    let cached_count = routes
        .iter()
        .filter(|route| checkpoint.contains_route(route.id))
        .count();
    println!(
        "规划 {} 路 LLM 分析（checkpoint 命中 {} 路）...\n",
        route_count, cached_count
    );

    // Step 3: 并发执行 LLM 分析
    let semaphore = Arc::new(Semaphore::new(LLM_CONCURRENCY));
    let mut handles = Vec::new();

    for route in routes
        .into_iter()
        .filter(|route| !checkpoint.contains_route(route.id))
    {
        let sem = semaphore.clone();
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let content = llm_with_retry_policy(
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
            .map_err(|error| (route.id, route.title.clone(), error.to_string()))?;
            Ok::<RouteResult, (usize, String, String)>(RouteResult {
                route_id: route.id,
                route_title: route.title,
                content,
            })
        });
        handles.push(handle);
    }

    let mut completed_results = Vec::new();
    let mut route_errors = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(result)) => {
                eprintln!("  ✓ Route {}: {}", result.route_id, result.route_title);
                completed_results.push(result);
            }
            Ok(Err(error)) => {
                eprintln!("  ✗ Route {}: {}: {}", error.0, error.1, error.2);
                route_errors.push(error);
            }
            Err(error) => {
                route_errors.push((0, "worker panic".into(), error.to_string()));
            }
        }
    }

    if !completed_results.is_empty() {
        checkpoint.extend(completed_results);
        checkpoint.save()?;
        println!(
            "已保存 {}/{} 路中间结果: {}",
            checkpoint.route_results.len(),
            route_count,
            InsightsCheckpoint::path()?.display()
        );
    }

    if !route_errors.is_empty() {
        anyhow::bail!(
            "{} 路分析失败；已完成结果已 checkpoint，下次只重跑缺失路由",
            route_errors.len()
        );
    }

    if checkpoint.route_results.len() != route_count {
        anyhow::bail!(
            "路由结果不完整: {}/{}；拒绝生成残缺报告",
            checkpoint.route_results.len(),
            route_count
        );
    }

    println!(
        "\n{} 路分析完成，合并生成最终报告...\n",
        checkpoint.route_results.len()
    );

    // Step 4: 合并 + 最终报告
    let combined = merge_route_results(&checkpoint.route_results);
    let final_prompt = build_final_prompt(&combined, stats, options.with_prescription);

    let report = llm_with_retry_policy(
        &client,
        &final_prompt,
        INSIGHTS_SYSTEM_PROMPT,
        final_report_retry_policy(),
        |attempt, max_retries, delay_secs, _err| {
            eprintln!(
                "    ⏳ 重试 ({}/{}) 等待 {}s...",
                attempt, max_retries, delay_secs
            );
        },
    )
    .await
    .map_err(|e| {
        anyhow::anyhow!(
            "最终报告生成失败: {}；10 路中间结果已保留，下次将直接重试合并",
            e
        )
    })?;

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

    InsightsCheckpoint::clear()?;

    println!("\n报告已保存 (ID: {})", doc.id());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{final_report_retry_policy, FINAL_REPORT_REQUEST_TIMEOUT_MILLIS};

    #[test]
    fn final_report_allows_slow_large_synthesis_requests() {
        let policy = final_report_retry_policy();
        assert_eq!(policy.request_timeout_millis, 300_000);
        assert_eq!(
            policy.request_timeout_millis,
            FINAL_REPORT_REQUEST_TIMEOUT_MILLIS
        );
    }
}
