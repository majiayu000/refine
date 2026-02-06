use anyhow::{anyhow, Result};
use refine_core::infra::{ClaudeClient, LlmClient, OpenAIClient};
use refine_core::knowledge::{Item, ItemType};
use std::path::{Path, PathBuf};

pub fn get_db_path(db: &str) -> PathBuf {
    if db.starts_with("~/") {
        dirs::home_dir()
            .map(|home| home.join(&db[2..]))
            .unwrap_or_else(|| PathBuf::from(db))
    } else {
        PathBuf::from(db)
    }
}

pub fn ensure_db_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn parse_item_type(raw: &str) -> Option<ItemType> {
    match raw.to_lowercase().as_str() {
        "knowledge" => Some(ItemType::Knowledge),
        "skill" => Some(ItemType::Skill),
        "snippet" => Some(ItemType::Snippet),
        _ => None,
    }
}

pub fn build_llm_client_from_env() -> Result<Box<dyn LlmClient>> {
    if let Some(api_key) = env_var(&["REFINE_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"]) {
        let mut client = ClaudeClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_ANTHROPIC_MODEL"]) {
            client = client.with_model(&model);
        }
        return Ok(Box::new(client));
    }

    if let Some(api_key) = env_var(&["REFINE_OPENAI_API_KEY", "OPENAI_API_KEY"]) {
        let mut client = OpenAIClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_OPENAI_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_OPENAI_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Ok(Box::new(client));
    }

    Err(anyhow!(
        "未配置 LLM API Key，请设置 REFINE_ANTHROPIC_API_KEY 或 REFINE_OPENAI_API_KEY"
    ))
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

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}
