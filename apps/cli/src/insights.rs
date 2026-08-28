//! insights v2 — 两级分析：本地聚类 + 10 路 LLM 并发

use crate::insights_checkpoint::{DatasetSignature, InsightsCheckpoint, CHECKPOINT_VERSION};
use crate::insights_manifest::{
    build_delta_summary, build_manifest, build_window_manifest, manifest_identity, render_manifest,
    rotation_seed, route_plan_identity, source_revision, EventTimeWindow, InsightsManifest,
};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use refine_core::infra::{llm_with_retry_policy, LlmClient, LlmRetryPolicy};
use refine_core::knowledge::{Document, DocumentRepository, ItemRepository};
use refine_core::session::{
    build_final_prompt_with_delta, cluster_session_observation_windows, format_data_quality_stats,
    merge_route_results_with_budget, plan_routes, DataQualityStats, RouteResult,
    INSIGHTS_SYSTEM_PROMPT, ROUTE_SYSTEM_PROMPT,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

const DEFAULT_LLM_CONCURRENCY: usize = 4;
const FINAL_REPORT_REQUEST_TIMEOUT_MILLIS: u64 = 300_000;
const INSIGHTS_PROMPT_IDENTITY: &str = "insights-v2:route-v2:delta-final-v4";
const FINAL_ROUTE_CONTEXT_CHARS: usize = 24_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowDisposition {
    NoData,
    PersistDeterministic,
    Analyze,
}

fn window_disposition(
    current_observations: usize,
    previous_observations: usize,
    current_eligible: usize,
) -> WindowDisposition {
    if current_observations == 0 && previous_observations == 0 {
        WindowDisposition::NoData
    } else if current_eligible == 0 {
        WindowDisposition::PersistDeterministic
    } else {
        WindowDisposition::Analyze
    }
}

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

async fn persist_report(
    persisted_report: &str,
    all_snapshot: bool,
    doc_store: &dyn DocumentRepository,
) -> Result<()> {
    let mut doc = Document::new("session-insights-v2", persisted_report);
    let kind = if all_snapshot { "Snapshot" } else { "Delta" };
    doc.set_title(&format!(
        "Session Insights v2 {} {}",
        kind,
        doc.created_at().format("%Y-%m-%d %H:%M")
    ));
    doc.set_url(&format!(
        "insights-v2://{}/{}",
        kind.to_lowercase(),
        doc.created_at().to_rfc3339()
    ));
    doc_store.save(&doc).await.context("保存报告失败")?;
    InsightsCheckpoint::clear()?;
    println!("\n报告已保存 (ID: {})", doc.id());
    Ok(())
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
    // Loading and classifying the windows is deterministic. Preserve that path
    // when no LLM is configured so inactivity/removal reports can still land.
    let llm_identity = llm_client
        .as_ref()
        .map(|client| client.cache_identity())
        .unwrap_or_else(|| "unknown".to_string());
    let source_revision = source_revision();
    let cutoff = InsightsCheckpoint::reusable_cutoff(
        options.period,
        options.with_prescription,
        &llm_identity,
        INSIGHTS_PROMPT_IDENTITY,
        &source_revision,
    )?
    .unwrap_or_else(Utc::now);
    let snapshot = item_store
        .load_observation_window_snapshot(cutoff, options.period)
        .await
        .context("在同一 SQLite snapshot 中加载 insights 窗口与来源元数据失败")?;
    let (current_window, previous_window, report_mode) = match options.period {
        Some(days) => {
            let days = i64::try_from(days).context("--period 超出支持范围")?;
            let current_start = cutoff - Duration::days(days);
            let previous_start = current_start - Duration::days(days);
            (
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
            EventTimeWindow {
                start: None,
                end: Some(cutoff),
            },
            None,
            "all-history-snapshot".to_string(),
        ),
    };

    println!(
        "加载 current={} / previous={} 条观测数据...",
        snapshot.current.len(),
        snapshot.previous.len()
    );

    // Step 1: 本地聚类（纯 Rust，无 LLM）
    let document_sources: HashMap<String, String> = snapshot
        .documents
        .iter()
        .map(|document| (document.id.as_str().to_string(), document.source.clone()))
        .collect();
    let mut comparison_windows = vec![snapshot.current.as_slice()];
    if options.period.is_some() {
        comparison_windows.push(snapshot.previous.as_slice());
    }
    let mut cohorts = cluster_session_observation_windows(&comparison_windows, &document_sources);
    let current_cohort = cohorts.remove(0);
    let previous_cohort = cohorts.pop();
    let cluster_result = &current_cohort.cluster;
    let previous_cluster = previous_cohort.as_ref().map(|cohort| &cohort.cluster);
    let stats = &cluster_result.global_stats;
    let quality = &cluster_result.data_quality;
    let disposition = window_disposition(
        snapshot.current.len(),
        snapshot.previous.len(),
        quality.eligible_observations,
    );
    if disposition == WindowDisposition::NoData {
        anyhow::bail!(
            "NO_DATA: 当前窗口与前一等长窗口均无 Observation；未生成 Session Insights 报告"
        );
    }
    let routes = if disposition == WindowDisposition::Analyze {
        plan_routes(cluster_result)
    } else {
        Vec::new()
    };
    let route_identity = route_plan_identity(&routes);

    println!(
        "Cohort: {}\nWindow: {}\n",
        format_data_quality_stats(quality),
        insights_window_label(options.period),
    );

    let current_manifest = build_window_manifest(
        current_window,
        &current_cohort.cohort_items,
        &snapshot.current,
        cluster_result,
        &snapshot.documents,
    )?;
    let previous_manifest = match (previous_window, previous_cohort.as_ref()) {
        (Some(window), Some(cohort)) => Some(build_window_manifest(
            window,
            &cohort.cohort_items,
            &snapshot.previous,
            &cohort.cluster,
            &snapshot.documents,
        )?),
        _ => None,
    };
    let manifest = build_manifest(
        &report_mode,
        cutoff,
        current_manifest,
        previous_manifest,
        llm_identity.clone(),
        INSIGHTS_PROMPT_IDENTITY,
        route_identity,
    );
    let manifest_identity = manifest_identity(&manifest)?;
    let delta_summary = build_delta_summary(
        cluster_result,
        &manifest.current_window,
        previous_cluster,
        manifest.previous_window.as_ref(),
    );
    if disposition == WindowDisposition::PersistDeterministic {
        let deterministic = "# Session Insights Report\n\n## 稳定基线\n\n当前窗口没有可用于 LLM 路由分析的 eligible sessions；本报告仅保存可复现 manifest 与跨窗 inactivity/evidence-gap 事实。";
        let persisted = report_with_metadata(
            deterministic,
            options.period,
            quality,
            &manifest,
            &delta_summary,
        )?;
        println!("{}", persisted);
        return persist_report(&persisted, options.all_snapshot, doc_store.as_ref()).await;
    }

    let client = llm_client
        .context("LLM_UNAVAILABLE: 当前窗口有 eligible sessions，但未配置 LLM；未生成报告")?;

    println!(
        "聚类完成: {} 个项目, {} sessions, {} decisions, {} bugs\n",
        stats.project_ranking.len(),
        stats.total_sessions,
        stats.total_decisions,
        stats.total_bugfixes,
    );

    let latest_updated_at = snapshot
        .current
        .iter()
        .map(|item| item.updated_at())
        .max()
        .context("Observation 集合缺少更新时间")?;
    let signature = DatasetSignature {
        checkpoint_version: CHECKPOINT_VERSION,
        observation_count: snapshot.current.len(),
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
        binary_identity: manifest.binary_identity.clone(),
        route_identity: manifest.route_identity.clone(),
        data_quality: quality.clone(),
    };
    let mut checkpoint = InsightsCheckpoint::load_matching(signature)?;

    // Step 2: 执行已纳入 manifest identity 的 LLM 分析路由
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

    persist_report(&persisted_report, options.all_snapshot, doc_store.as_ref()).await
}

#[cfg(test)]
mod tests {
    use super::{parse_llm_concurrency, window_disposition, WindowDisposition};

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

    #[test]
    fn previous_only_window_persists_deterministic_inactivity() {
        assert_eq!(
            window_disposition(0, 12, 0),
            WindowDisposition::PersistDeterministic
        );
        assert_eq!(window_disposition(0, 0, 0), WindowDisposition::NoData);
        assert_eq!(window_disposition(4, 12, 4), WindowDisposition::Analyze);
    }
}
