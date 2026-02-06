use refine_core::infra::{ClaudeClient, LlmClient, OpenAIClient, SqliteStore};
use refine_core::knowledge::{Item, ItemRepository, Source};
use refine_core::refinement::{Conversation, ExtractionPolicy, Extractor, PromptTemplate};
use serde::Deserialize;
use std::sync::Arc;
use tokio::runtime::Runtime;

const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";

#[derive(Debug, Clone)]
pub(super) struct IngestRequest {
    pub content: String,
    pub url: String,
    pub source: String,
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExtractRequest {
    content: String,
    url: String,
    source: String,
    title: Option<String>,
}

pub(super) fn handle_extract(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Result<Vec<String>, String> {
    let req = parse_request_body(request)?;
    ingest_conversation(
        store,
        runtime,
        llm_client,
        IngestRequest {
            content: req.content,
            url: req.url,
            source: req.source,
            title: req.title,
        },
    )
}

pub(super) fn ingest_conversation(
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    runtime.block_on(extract_and_store(store, llm_client.map(Arc::clone), req))
}

pub(super) fn build_llm_client_from_env() -> Result<Arc<dyn LlmClient>, String> {
    if let Some(api_key) = env_var(&["REFINE_ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY"]) {
        let mut client = ClaudeClient::new(&api_key);
        if let Some(model) = env_var(&["REFINE_ANTHROPIC_MODEL"]) {
            client = client.with_model(&model);
        }
        if let Some(base_url) = env_var(&["REFINE_ANTHROPIC_BASE_URL"]) {
            client = client.with_base_url(&base_url);
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
    llm_client: Option<Arc<dyn LlmClient>>,
    req: IngestRequest,
) -> Result<Vec<String>, String> {
    let mut items = build_items(llm_client, &req).await;
    if items.is_empty() {
        items.push(fallback_item(&req));
    }

    let mut ids = Vec::with_capacity(items.len());
    for mut item in items {
        item.set_source(Source::new(&req.source).with_url(&req.url));
        if item.content().trim().is_empty() {
            item.set_content(&req.content);
        }
        store.save(&item).await.map_err(|e| e.to_string())?;
        ids.push(item.id().to_string());
    }

    Ok(ids)
}

async fn build_items(llm_client: Option<Arc<dyn LlmClient>>, req: &IngestRequest) -> Vec<Item> {
    let Some(client) = llm_client else {
        return vec![fallback_item(req)];
    };

    let conversation = match Conversation::parse(&req.content) {
        Ok(conversation) => conversation,
        Err(err) => {
            tracing::warn!("conversation parse failed, fallback extraction: {}", err);
            return vec![fallback_item(req)];
        }
    };

    let policy = ExtractionPolicy::default();
    let prompt = PromptTemplate::extraction_prompt(&conversation.raw, &policy);
    let llm_response = match client
        .complete(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!("llm extraction failed, fallback extraction: {}", err);
            return vec![fallback_item(req)];
        }
    };

    let extractor = Extractor::new(policy);
    let extraction = match extractor.parse_response(&llm_response, &conversation) {
        Ok(extraction) => extraction,
        Err(err) => {
            tracing::warn!("extraction parse failed, fallback extraction: {}", err);
            return vec![fallback_item(req)];
        }
    };

    if extraction.items.is_empty() {
        return vec![fallback_item(req)];
    }

    extraction.items
}

fn fallback_item(req: &IngestRequest) -> Item {
    let default_title = format!("[{}] 对话提炼", req.source);
    let title = req
        .title
        .as_ref()
        .map(|value| trim_to(value, 120))
        .unwrap_or_else(|| trim_to(&default_title, 120));

    let summary = build_summary(&req.content, 200);
    let mut item = Item::new_knowledge(&title, &summary);
    item.set_content(&req.content);
    item
}

fn build_summary(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "No content".to_string();
    }
    trim_to(&normalized, max_chars)
}

fn trim_to(input: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            break;
        }
        output.push(ch);
    }
    if output.len() < input.len() {
        format!("{}...", output.trim_end())
    } else {
        output
    }
}

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}
