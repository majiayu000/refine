//! insights v2 — 两级分析：本地聚类 + 10 路 LLM 并发

use crate::insights_checkpoint::{DatasetSignature, InsightsCheckpoint, CHECKPOINT_VERSION};
use crate::insights_manifest::{
    build_delta_summary, build_manifest, build_window_manifest, manifest_identity, render_manifest,
    rotation_seed, source_revision, EventTimeWindow, InsightsManifest,
};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use refine_core::infra::{llm_with_retry_policy, LlmClient, LlmRetryPolicy};
use refine_core::knowledge::{Document, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_final_prompt_with_delta, cluster_observations, format_data_quality_stats,
    merge_route_results_with_budget, plan_routes, DataQualityStats, RouteResult,
    INSIGHTS_SYSTEM_PROMPT, ROUTE_SYSTEM_PROMPT,
};
use std::sync::Arc;
use tokio::sync::Semaphore;

const DEFAULT_LLM_CONCURRENCY: usize = 4;
const FINAL_REPORT_REQUEST_TIMEOUT_MILLIS: u64 = 300_000;
const INSIGHTS_PROMPT_IDENTITY: &str = "insights-v2:route-v2:delta-final-v3";
const FINAL_ROUTE_CONTEXT_CHARS: usize = 24_000;

fn insights_window_label(period: Option<usize>) -> String {
    match period {
        Some(days) => format!("rolling {days} days (event time)"),
        None => "all available observations".to_string(),
    }
}

fn report_with_metadata(
    report: &str,
    period: Option<usize>,
    quality: &DataQualityStats,
    manifest: &InsightsManifest,
    delta_summary: &str,
) -> Result<String> {
    Ok(format!(
        "{}\n\n> Window: {}\n> Cohort: linked observations excluding unattended/subagent documents\n> Data quality: {}\n\n## 本期变化（deterministic）\n\n{}\n\n{}",
        render_manifest(manifest)?,
        insights_window_label(period),
        format_data_quality_stats(quality),
        delta_summary,
        report,
    ))
}

fn llm_concurrency() -> Result<usize> {
    match std::env::var("REFINE_INSIGHTS_CONCURRENCY") {
        Ok(raw) => parse_llm_concurrency(&raw),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_LLM_CONCURRENCY),
        Err(error) => Err(anyhow::anyhow!(
            "failed to read REFINE_INSIGHTS_CONCURRENCY: {error}"
        )),
    }
}

fn parse_llm_concurrency(raw: &str) -> Result<usize> {
    raw.trim()
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow::anyhow!("REFINE_INSIGHTS_CONCURRENCY must be a positive integer"))
}

fn final_report_retry_policy() -> LlmRetryPolicy {
    LlmRetryPolicy {
        request_timeout_millis: FINAL_REPORT_REQUEST_TIMEOUT_MILLIS,
        ..LlmRetryPolicy::default()
    }
}

#[allow(dead_code)]
pub struct InsightsOptions {
    pub period: Option<usize>,
    pub all_snapshot: bool,
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
    if options.period.is_none() != options.all_snapshot {
        anyhow::bail!("内部参数错误：全历史报告必须由显式 --all 选择");
    }
    let client = match &llm_client {
        Some(c) => c.clone(),
        None => {
            println!("insights 需要 LLM。请配置 API Key。");
            return Ok(());
        }
    };

    let llm_identity = client.cache_identity();
    let source_revision = source_revision();
    let cutoff = InsightsCheckpoint::reusable_cutoff(
        options.period,
        options.with_prescription,
        &llm_identity,
        INSIGHTS_PROMPT_IDENTITY,
        &source_revision,
    )?
    .unwrap_or_else(Utc::now);
    let (observations, previous_observations, current_window, previous_window, report_mode) =
        match options.period {
            Some(days) => {
                let days = i64::try_from(days).context("--period 超出支持范围")?;
                let current_start = cutoff - Duration::days(days);
                let previous_start = current_start - Duration::days(days);
                let current = item_store
                    .find_observations_by_event_range(current_start, cutoff)
                    .await
                    .context("加载当前 Observation 窗口失败")?;
                let previous = item_store
                    .find_observations_by_event_range(previous_start, current_start)
                    .await
                    .context("加载前一 Observation 窗口失败")?;
                (
                    current,
                    Some(previous),
                    EventTimeWindow {
                        start: Some(current_start),
                        end: Some(cutoff),
                    },
                    Some(EventTimeWindow {
                        start: Some(previous_start),
                        end: Some(current_start),
                    }),
                    format!("rolling-{days}d-delta"),
                )
            }
            None => (
                item_store
                    .find_by_type(refine_core::knowledge::ItemType::Observation)
                    .await
                    .context("加载全历史 Observation snapshot 失败")?,
                None,
                EventTimeWindow {
                    start: None,
                    end: Some(cutoff),
                },
                None,
                "all-history-snapshot".to_string(),
            ),
        };

    if observations.is_empty() {
        println!("暂无观测数据。请先运行 `refine ingest-sessions` 导入会话。");
        return Ok(());
    }

    println!("加载 {} 条观测数据...", observations.len());

    // Step 1: 本地聚类（纯 Rust，无 LLM）
    let cluster_result = cluster_observations(&observations);
    let previous_cluster = previous_observations
        .as_ref()
        .map(|items| cluster_observations(items));
    let stats = &cluster_result.global_stats;
    let quality = &cluster_result.data_quality;

    println!(
        "Cohort: {}\nWindow: {}\n",
        format_data_quality_stats(quality),
        insights_window_label(options.period),
    );

    let current_manifest = build_window_manifest(
        current_window,
        &observations,
        &cluster_result,
        doc_store.as_ref(),
    )
    .await?;
    let previous_manifest = match (
        previous_window,
        previous_observations.as_ref(),
        previous_cluster.as_ref(),
    ) {
        (Some(window), Some(items), Some(cluster)) => {
            Some(build_window_manifest(window, items, cluster, doc_store.as_ref()).await?)
        }
        _ => None,
    };
    let manifest = build_manifest(
        &report_mode,
        cutoff,
        current_manifest,
        previous_manifest,
        llm_identity.clone(),
        INSIGHTS_PROMPT_IDENTITY,
    );
    let manifest_identity = manifest_identity(&manifest)?;
    let delta_summary = build_delta_summary(
        &cluster_result,
        &manifest.current_window,
        previous_cluster.as_ref(),
        manifest.previous_window.as_ref(),
    );
    if quality.eligible_observations == 0 {
        anyhow::bail!(
            "没有可用于分析的已关联交互观测（输入 {}，脱链 {}，模式排除 {}）；拒绝生成空或错口径报告",
            quality.input_observations,
            quality.detached_observations,
            quality.mode_excluded_observations,
        );
    }

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
        llm_identity,
        prompt_identity: INSIGHTS_PROMPT_IDENTITY.to_string(),
        window_start: manifest.current_window.event_time.start,
        window_end: manifest.current_window.event_time.end,
        event_time_cutoff: Some(cutoff),
        previous_cohort_identity: manifest
            .previous_window
            .as_ref()
            .map(|window| window.cohort_identity.clone()),
        manifest_identity,
        source_revision: manifest.source_revision.clone(),
        data_quality: quality.clone(),
    };
    let mut checkpoint = InsightsCheckpoint::load_matching(signature)?;

    // Step 2: 规划 LLM 分析路由
    let routes = plan_routes(&cluster_result);
    let route_count = routes.len();
    let cached_count = routes
        .iter()
        .filter(|route| checkpoint.contains_route(route.id))
        .count();
    let llm_concurrency = llm_concurrency()?;
    println!(
        "规划 {} 路 LLM 分析（checkpoint 命中 {} 路，最大并发 {}）...\n",
        route_count, cached_count, llm_concurrency
    );

    // Step 3: 并发执行 LLM 分析
    let semaphore = Arc::new(Semaphore::new(llm_concurrency));
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
    let combined = merge_route_results_with_budget(
        &checkpoint.route_results,
        FINAL_ROUTE_CONTEXT_CHARS,
        rotation_seed(&quality.cohort_identity),
    );
    let final_prompt = build_final_prompt_with_delta(
        &combined,
        stats,
        quality,
        Some(&delta_summary),
        options.with_prescription,
    );

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

    let persisted_report =
        report_with_metadata(&report, options.period, quality, &manifest, &delta_summary)?;
    println!("{}", persisted_report);

    // Step 5: 保存
    let mut doc = Document::new("session-insights-v2", &persisted_report);
    let title = format!(
        "Session Insights v2 {} {}",
        if options.all_snapshot {
            "Snapshot"
        } else {
            "Delta"
        },
        doc.created_at().format("%Y-%m-%d %H:%M")
    );
    doc.set_title(&title);
    doc.set_url(&format!(
        "insights-v2://{}/{}",
        if options.all_snapshot {
            "snapshot"
        } else {
            "delta"
        },
        doc.created_at().to_rfc3339()
    ));
    doc_store.save(&doc).await.context("保存报告失败")?;

    InsightsCheckpoint::clear()?;

    println!("\n报告已保存 (ID: {})", doc.id());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_llm_concurrency;

    #[test]
    fn insights_concurrency_requires_a_positive_integer() {
        assert_eq!(parse_llm_concurrency(" 3 ").unwrap(), 3);
        assert!(parse_llm_concurrency("0").is_err());
        assert!(parse_llm_concurrency("many").is_err());
    }

    #[test]
    fn insights_default_concurrency_is_provider_friendly() {
        assert_eq!(super::DEFAULT_LLM_CONCURRENCY, 4);
    }
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
