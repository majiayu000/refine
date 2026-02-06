use refine_core::infra::{ClaudeClient, LlmClient, OpenAIClient, SqliteStore};
use refine_core::knowledge::{ItemRepository, Source};
use refine_core::refinement::{Conversation, ExtractionPolicy, Extractor, PromptTemplate};
use serde::Deserialize;
use std::sync::Arc;
use tokio::runtime::Runtime;

const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";

#[derive(Debug, Deserialize)]
struct ExtractRequest {
    content: String,
    url: String,
    source: String,
}

pub(super) fn handle_extract(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Result<Vec<String>, String> {
    let req = parse_request_body(request)?;

    let llm_client = match llm_client {
        Some(client) => Arc::clone(client),
        None => {
            return Err(
                "LLM not configured. Set REFINE_ANTHROPIC_API_KEY or REFINE_OPENAI_API_KEY."
                    .to_string(),
            );
        }
    };

    runtime.block_on(extract_and_store(store, llm_client, req))
}

pub(super) fn build_llm_client_from_env() -> Result<Arc<dyn LlmClient>, String> {
    if let Some(api_key) = env_var(&["REFINE_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"]) {
        let mut client = ClaudeClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_ANTHROPIC_MODEL"]) {
            client = client.with_model(&model);
        }
        return Ok(Arc::new(client));
    }

    if let Some(api_key) = env_var(&["REFINE_OPENAI_API_KEY", "OPENAI_API_KEY"]) {
        let mut client = OpenAIClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_OPENAI_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_OPENAI_BASE_URL"]) {
            client = client.with_base_url(&base_url);
        }
        return Ok(Arc::new(client));
    }

    Err("missing API key".to_string())
}

fn parse_request_body(request: &mut tiny_http::Request) -> Result<ExtractRequest, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|_| "Failed to read request body".to_string())?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON: {}", e))
}

async fn extract_and_store(
    store: &Arc<SqliteStore>,
    llm_client: Arc<dyn LlmClient>,
    req: ExtractRequest,
) -> Result<Vec<String>, String> {
    let conversation = Conversation::parse(&req.content).map_err(|e| e.to_string())?;
    let policy = ExtractionPolicy::default();
    let prompt = PromptTemplate::extraction_prompt(&conversation.raw, &policy);

    let llm_response = llm_client
        .complete(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await
        .map_err(|e| e.to_string())?;

    let extractor = Extractor::new(policy);
    let extraction = extractor
        .parse_response(&llm_response, &conversation)
        .map_err(|e| e.to_string())?;

    if extraction.items.is_empty() {
        return Err("No items extracted from conversation".to_string());
    }

    let mut ids = Vec::with_capacity(extraction.items.len());
    for mut item in extraction.items {
        item.set_source(Source::new(&req.source).with_url(&req.url));
        if item.content().trim().is_empty() {
            item.set_content(&req.content);
        }
        store.save(&item).await.map_err(|e| e.to_string())?;
        ids.push(item.id().to_string());
    }

    Ok(ids)
}

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}
