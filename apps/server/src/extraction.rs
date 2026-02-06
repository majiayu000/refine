use refine_core::infra::LlmClient;
use refine_core::knowledge::{Item, ItemRepository, Source};
use refine_core::refinement::{
    Conversation, ExtractionPolicy, ExtractionResult, Extractor, PromptTemplate,
};
use std::sync::Arc;

use crate::models::{now_iso, ConversationRecord, ConversationStatus, ExtractionMode, JobStatus};
use crate::state::AppState;

const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";
const JSON_REPAIR_SYSTEM_PROMPT: &str =
    "你是 JSON 修复器。只输出一个合法 JSON 对象，不要输出 markdown 或解释。";

pub fn spawn_extraction(
    state: Arc<AppState>,
    conversation_id: String,
    job_id: String,
    mode: ExtractionMode,
) {
    tokio::spawn(async move {
        if let Err(err) = run_extraction(state.clone(), &conversation_id, &job_id, mode).await {
            set_conversation_failed(&state, &conversation_id, &err).await;
            set_job_failed(&state, &job_id, &err).await;
        }
    });
}

async fn run_extraction(
    state: Arc<AppState>,
    conversation_id: &str,
    job_id: &str,
    mode: ExtractionMode,
) -> Result<(), String> {
    set_job_running(&state, job_id).await;
    set_conversation_status(&state, conversation_id, ConversationStatus::Processing).await;

    let conversation = {
        let guard = state.conversations.read().await;
        guard
            .get(conversation_id)
            .cloned()
            .ok_or_else(|| "Conversation not found".to_string())?
    };

    let mut items = build_items(&state, &conversation, mode).await?;
    if items.is_empty() {
        items.push(fallback_item(&conversation));
    }

    let mut item_ids = Vec::with_capacity(items.len());
    for item in &mut items {
        item.set_source(
            Source::new(&conversation.source)
                .with_conversation_id(&conversation.id)
                .with_url(&conversation.url),
        );
        if item.content().trim().is_empty() {
            item.set_content(&conversation.raw_content);
        }
        state.store.save(item).await.map_err(|e| e.to_string())?;
        item_ids.push(item.id().to_string());
    }

    set_conversation_processed(&state, conversation_id, item_ids).await;
    set_job_succeeded(&state, job_id).await;

    Ok(())
}

async fn build_items(
    state: &Arc<AppState>,
    conversation: &ConversationRecord,
    mode: ExtractionMode,
) -> Result<Vec<Item>, String> {
    let llm_client = match &state.llm_client {
        Some(client) => client.clone(),
        None => return Ok(vec![fallback_item(conversation)]),
    };

    let parsed = Conversation::parse(&conversation.raw_content).map_err(|e| e.to_string())?;
    let policy = mode_to_policy(mode);
    let prompt = PromptTemplate::extraction_prompt(&parsed.raw, &policy);
    let llm_response = llm_client
        .complete(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await
        .map_err(|e| e.to_string())?;

    let extractor = Extractor::new(policy);
    let extraction =
        match parse_extraction_with_repair(llm_client.clone(), &extractor, &parsed, &llm_response)
            .await
        {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!("提炼 JSON 解析失败，降级为 fallback item: {}", err);
                return Ok(vec![fallback_item(conversation)]);
            }
        };

    if extraction.items.is_empty() {
        Ok(vec![fallback_item(conversation)])
    } else {
        Ok(extraction.items)
    }
}

fn mode_to_policy(mode: ExtractionMode) -> ExtractionPolicy {
    match mode {
        ExtractionMode::Auto => ExtractionPolicy::default(),
        ExtractionMode::Knowledge => ExtractionPolicy::knowledge_only(),
        ExtractionMode::Snippet => ExtractionPolicy::snippets_only(),
        ExtractionMode::Skill => ExtractionPolicy {
            extract_knowledge: false,
            extract_skills: true,
            extract_snippets: false,
            ..ExtractionPolicy::default()
        },
    }
}

async fn parse_extraction_with_repair(
    llm_client: Arc<dyn LlmClient>,
    extractor: &Extractor,
    conversation: &Conversation,
    raw_response: &str,
) -> Result<ExtractionResult, String> {
    match extractor.parse_response(raw_response, conversation) {
        Ok(extraction) => Ok(extraction),
        Err(first_err) => {
            let first_message = first_err.to_string();
            tracing::warn!("首次提炼解析失败，尝试 JSON 修复重试: {}", first_message);

            let repair_prompt = build_json_repair_prompt(raw_response, &first_message);
            let repaired_response = llm_client
                .complete(&repair_prompt, Some(JSON_REPAIR_SYSTEM_PROMPT))
                .await
                .map_err(|e| format!("原始解析失败，且 JSON 修复请求失败: {}", e))?;

            extractor
                .parse_response(&repaired_response, conversation)
                .map_err(|second_err| {
                    format!(
                        "原始解析失败: {}; JSON 修复后仍失败: {}",
                        first_message, second_err
                    )
                })
        }
    }
}

fn build_json_repair_prompt(raw_response: &str, parse_error: &str) -> String {
    format!(
        r#"你会收到一段本应是 JSON 的文本，但它存在语法错误。
请将它修复为合法 JSON，并严格满足以下要求：
1) 只能输出一个 JSON 对象；
2) 顶层字段必须是 "items"；
3) "items" 必须是数组，数组元素结构为:
   {{
     "type": "knowledge|skill|snippet",
     "title": "...",
     "summary": "...",
     "content": "...",
     "tags": ["..."]
   }}
4) 若无法可靠修复，请返回 {{"items":[]}}；
5) 不要输出 markdown 代码块，不要输出任何解释文字。

原始解析错误:
{}

待修复文本:
{}
"#,
        parse_error, raw_response
    )
}

fn fallback_item(conversation: &ConversationRecord) -> Item {
    let default_title = format!("[{}] {}", conversation.source, conversation.captured_at);
    let title = conversation
        .title
        .as_ref()
        .map(|v| trim_to(v, 120))
        .unwrap_or_else(|| trim_to(&default_title, 120));

    let summary = build_summary(&conversation.raw_content, 200);
    let mut item = Item::new_knowledge(&title, &summary);
    item.set_content(&conversation.raw_content);
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
    let mut out = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(ch);
    }
    if out.len() < input.len() {
        format!("{}...", out.trim_end())
    } else {
        out
    }
}

async fn set_job_running(state: &Arc<AppState>, job_id: &str) {
    let job_snapshot = {
        let mut jobs = state.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Running;
            job.updated_at = now_iso();
            job.error = None;
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.persistence.upsert_job(&job) {
            tracing::warn!("persist running job failed: {}", err);
        }
    }
}

async fn set_job_succeeded(state: &Arc<AppState>, job_id: &str) {
    let job_snapshot = {
        let mut jobs = state.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Succeeded;
            job.updated_at = now_iso();
            job.error = None;
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.persistence.upsert_job(&job) {
            tracing::warn!("persist succeeded job failed: {}", err);
        }
    }
}

async fn set_job_failed(state: &Arc<AppState>, job_id: &str, error: &str) {
    let job_snapshot = {
        let mut jobs = state.jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::Failed;
            job.updated_at = now_iso();
            job.error = Some(error.to_string());
            Some(job.clone())
        } else {
            None
        }
    };
    if let Some(job) = job_snapshot {
        if let Err(err) = state.persistence.upsert_job(&job) {
            tracing::warn!("persist failed job failed: {}", err);
        }
    }
}

async fn set_conversation_status(
    state: &Arc<AppState>,
    conversation_id: &str,
    status: ConversationStatus,
) {
    let conversation_snapshot = {
        let mut conversations = state.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = status;
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.persistence.upsert_conversation(&conversation) {
            tracing::warn!("persist conversation status failed: {}", err);
        }
    }
}

async fn set_conversation_processed(
    state: &Arc<AppState>,
    conversation_id: &str,
    item_ids: Vec<String>,
) {
    let conversation_snapshot = {
        let mut conversations = state.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = ConversationStatus::Processed;
            conversation.item_ids = item_ids;
            conversation.last_error = None;
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.persistence.upsert_conversation(&conversation) {
            tracing::warn!("persist processed conversation failed: {}", err);
        }
    }
}

async fn set_conversation_failed(state: &Arc<AppState>, conversation_id: &str, error: &str) {
    let conversation_snapshot = {
        let mut conversations = state.conversations.write().await;
        if let Some(conversation) = conversations.get_mut(conversation_id) {
            conversation.status = ConversationStatus::Failed;
            conversation.last_error = Some(error.to_string());
            Some(conversation.clone())
        } else {
            None
        }
    };
    if let Some(conversation) = conversation_snapshot {
        if let Err(err) = state.persistence.upsert_conversation(&conversation) {
            tracing::warn!("persist failed conversation failed: {}", err);
        }
    }
}
