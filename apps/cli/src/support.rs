use anyhow::{anyhow, Result};
use refine_core::infra::{
    build_llm_client_from_env as build_core_llm_client_from_env, LlmClient,
};
use refine_core::knowledge::{Item, ItemType};
use std::sync::Arc;

pub fn parse_item_type(raw: &str) -> Option<ItemType> {
    match raw.to_lowercase().as_str() {
        "knowledge" => Some(ItemType::Knowledge),
        "skill" => Some(ItemType::Skill),
        "snippet" => Some(ItemType::Snippet),
        "observation" => Some(ItemType::Observation),
        _ => None,
    }
}

pub fn build_llm_client_from_env() -> Result<Arc<dyn LlmClient>> {
    build_core_llm_client_from_env().ok_or_else(|| {
        anyhow!("未配置 LLM API Key，请设置 REFINE_ANTHROPIC_API_KEY 或 REFINE_OPENAI_API_KEY")
    })
}

pub fn format_item(item: &Item, verbose: bool) -> String {
    if verbose {
        format!(
            "ID: {}\n类型: {:?}\n标题: {}\n摘要: {}\n标签: {}\n创建: {}\n---\n{}",
            item.id().as_str(),
            item.item_type(),
            item.title(),
            item.summary(),
            item.tags()
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            item.created_at().format("%Y-%m-%d %H:%M"),
            item.content()
        )
    } else {
        format!(
            "[{:?}] {} - {}",
            item.item_type(),
            item.id().as_str().chars().take(8).collect::<String>(),
            item.title()
        )
    }
}
