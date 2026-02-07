//! 提炼用例（应用层编排）
//!
//! 在不引入新 crate 的过渡阶段，将 LLM 调用、JSON 修复和 fallback 逻辑集中，
//! 供 server/desktop/cli 复用，避免多处重复实现。

use crate::error::{DomainError, DomainResult};
use crate::infra::LlmClient;
use crate::knowledge::Item;
use crate::refinement::{
    Conversation, ExtractionPolicy, ExtractionResult, Extractor, PromptTemplate,
};

pub const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";
pub const JSON_REPAIR_SYSTEM_PROMPT: &str =
    "你是 JSON 修复器。只输出一个合法 JSON 对象，不要输出 markdown 或解释。";

/// 使用 LLM 提炼 Item 列表。
///
/// 调用方可在失败时降级到 `build_fallback_item`。
pub async fn extract_items_with_llm(
    llm_client: &dyn LlmClient,
    raw_content: &str,
    policy: ExtractionPolicy,
) -> DomainResult<Vec<Item>> {
    let conversation = Conversation::parse(raw_content)?;
    let prompt = PromptTemplate::extraction_prompt(&conversation.raw, &policy);
    let llm_response = llm_client
        .complete(&prompt, Some(EXTRACTION_SYSTEM_PROMPT))
        .await
        .map_err(|err| DomainError::Extraction(format!("LLM 调用失败: {}", err)))?;

    let extractor = Extractor::new(policy);
    let extraction =
        parse_extraction_with_repair(llm_client, &extractor, &conversation, &llm_response).await?;

    if extraction.items.is_empty() {
        return Err(DomainError::Extraction("提炼结果为空".to_string()));
    }

    Ok(extraction.items)
}

/// 构建 fallback Item。
///
/// - `captured_at` 存在时，默认标题为 `[source] captured_at`
/// - 否则默认标题为 `[source] 对话提炼`
pub fn build_fallback_item(
    source: &str,
    title: Option<&str>,
    raw_content: &str,
    captured_at: Option<&str>,
) -> Item {
    let default_title = match captured_at.map(str::trim).filter(|v| !v.is_empty()) {
        Some(ts) => format!("[{}] {}", source, ts),
        None => format!("[{}] 对话提炼", source),
    };
    let title = title
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| trim_to(v, 120))
        .unwrap_or_else(|| trim_to(&default_title, 120));

    let summary = build_summary(raw_content, 200);
    let mut item = Item::new_knowledge(&title, &summary);
    item.set_content(raw_content);
    item
}

async fn parse_extraction_with_repair(
    llm_client: &dyn LlmClient,
    extractor: &Extractor,
    conversation: &Conversation,
    raw_response: &str,
) -> DomainResult<ExtractionResult> {
    match extractor.parse_response(raw_response, conversation) {
        Ok(extraction) => Ok(extraction),
        Err(first_err) => {
            let first_message = first_err.to_string();
            tracing::warn!("首次提炼解析失败，尝试 JSON 修复重试: {}", first_message);

            let repair_prompt = build_json_repair_prompt(raw_response, &first_message);
            let repaired_response = llm_client
                .complete(&repair_prompt, Some(JSON_REPAIR_SYSTEM_PROMPT))
                .await
                .map_err(|err| {
                    DomainError::Extraction(format!("原始解析失败，且 JSON 修复请求失败: {}", err))
                })?;

            extractor
                .parse_response(&repaired_response, conversation)
                .map_err(|second_err| {
                    DomainError::Extraction(format!(
                        "原始解析失败: {}; JSON 修复后仍失败: {}",
                        first_message, second_err
                    ))
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

fn build_summary(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "No content".to_string();
    }
    trim_to(&normalized, max_chars)
}

fn trim_to(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::InfraResult;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct SequenceLlmClient {
        responses: Arc<Mutex<Vec<String>>>,
    }

    impl SequenceLlmClient {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(
                    responses.into_iter().map(ToString::to_string).collect(),
                )),
            }
        }
    }

    #[async_trait]
    impl LlmClient for SequenceLlmClient {
        async fn complete(&self, _prompt: &str, _system: Option<&str>) -> InfraResult<String> {
            let mut guard = self
                .responses
                .lock()
                .map_err(|err| crate::error::InfraError::LlmRequest(err.to_string()))?;
            if guard.is_empty() {
                return Err(crate::error::InfraError::LlmRequest(
                    "no mock response".to_string(),
                ));
            }
            Ok(guard.remove(0))
        }
    }

    #[tokio::test]
    async fn extract_items_with_llm_works() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"T","summary":"S","content":"C","tags":["rust"]}]}"#,
        ]);
        let items = extract_items_with_llm(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "T");
    }

    #[tokio::test]
    async fn extract_items_with_llm_repairs_json() {
        let client = SequenceLlmClient::new(vec![
            r#"{"items":[{"type":"knowledge","title":"broken","summary":"bad","content":"oops "quote","tags":[]}]}"#,
            r#"{"items":[{"type":"skill","title":"fixed","summary":"ok","content":"done","tags":["a"]}]}"#,
        ]);
        let items = extract_items_with_llm(
            &client,
            "Human: hello\nAssistant: world",
            ExtractionPolicy::default(),
        )
        .await
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "fixed");
    }

    #[test]
    fn fallback_item_uses_timestamp_when_present() {
        let item =
            build_fallback_item("chatgpt", None, "hello world", Some("2026-02-06T00:00:00Z"));
        assert_eq!(item.title(), "[chatgpt] 2026-02-06T00:00:00Z");
    }

    #[test]
    fn fallback_item_uses_default_title_without_timestamp() {
        let item = build_fallback_item("claude", None, "hello world", None);
        assert_eq!(item.title(), "[claude] 对话提炼");
    }
}
