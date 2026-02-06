//! 本地 HTTP API 服务器
//!
//! 提供 HTTP API 供浏览器扩展调用

use refine_core::infra::{ClaudeClient, LlmClient, OpenAIClient, SqliteStore};
use refine_core::knowledge::{ItemRepository, Source};
use refine_core::refinement::{Conversation, ExtractionPolicy, Extractor, PromptTemplate};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tiny_http::{Header, Method, Response, Server};
use tokio::runtime::{Builder as RuntimeBuilder, Runtime};

const SERVER_PORT: u16 = 19527;
const CLIENT_HEADER_NAME: &str = "X-Refine-Client";
const CLIENT_HEADER_VALUE: &str = "extension";
const EXTRACTION_SYSTEM_PROMPT: &str =
    "你是 Refine 的知识提炼助手。严格按要求返回 JSON，不要输出额外说明文本。";

/// 提取请求
#[derive(Debug, Deserialize)]
struct ExtractRequest {
    content: String,
    url: String,
    source: String,
}

/// API 响应
#[derive(Debug, Serialize)]
struct ApiResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ids: Option<Vec<String>>,
}

/// 启动 HTTP 服务器
pub fn start_server(store: Arc<SqliteStore>) {
    std::thread::spawn(move || {
        let runtime = match RuntimeBuilder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create HTTP runtime: {}", e);
                return;
            }
        };

        let server = match Server::http(format!("127.0.0.1:{}", SERVER_PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to start HTTP server: {}", e);
                return;
            }
        };

        let llm_client = match build_llm_client_from_env() {
            Ok(client) => Some(client),
            Err(e) => {
                eprintln!("LLM is not configured, /extract will return errors: {}", e);
                None
            }
        };

        println!("Refine API: http://localhost:{}", SERVER_PORT);

        for mut request in server.incoming_requests() {
            let response = handle_request(&mut request, &store, &runtime, llm_client.as_ref());
            let _ = request.respond(response);
        }
    });
}

fn handle_request(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let origin = get_origin_header(request);
    let allowed_origin = origin
        .as_deref()
        .filter(|value| is_allowed_extension_origin(value))
        .map(str::to_string);

    // 处理 CORS 预检请求
    if request.method() == &Method::Options {
        if let Some(origin) = allowed_origin.as_deref() {
            return empty_response(204, Some(origin));
        }
        return json_response(
            403,
            &ApiResponse {
                success: false,
                message: Some("Forbidden origin".to_string()),
                ids: None,
            },
            None,
        );
    }

    let path = request.url().to_string();
    let method = request.method().clone();

    match (method, path.as_str()) {
        (Method::Get, "/health") => json_response(200, &handle_health(), allowed_origin.as_deref()),
        (Method::Post, "/extract") => {
            if !is_authorized_extension_request(request, allowed_origin.as_deref()) {
                return json_response(
                    403,
                    &ApiResponse {
                        success: false,
                        message: Some("Unauthorized extension request".to_string()),
                        ids: None,
                    },
                    allowed_origin.as_deref(),
                );
            }
            let result = handle_extract(request, store, runtime, llm_client);
            let status = if result.success { 200 } else { 400 };
            json_response(status, &result, allowed_origin.as_deref())
        }
        _ => json_response(
            404,
            &ApiResponse {
                success: false,
                message: Some("Not found".to_string()),
                ids: None,
            },
            allowed_origin.as_deref(),
        ),
    }
}

fn handle_health() -> ApiResponse {
    ApiResponse {
        success: true,
        message: Some("Refine is running".to_string()),
        ids: None,
    }
}

fn handle_extract(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> ApiResponse {
    // 读取请求体
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        return ApiResponse {
            success: false,
            message: Some("Failed to read request body".to_string()),
            ids: None,
        };
    }

    // 解析 JSON
    let req: ExtractRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return ApiResponse {
                success: false,
                message: Some(format!("Invalid JSON: {}", e)),
                ids: None,
            };
        }
    };

    let llm_client =
        match llm_client {
            Some(client) => client.clone(),
            None => return ApiResponse {
                success: false,
                message: Some(
                    "LLM not configured. Set REFINE_ANTHROPIC_API_KEY or REFINE_OPENAI_API_KEY."
                        .to_string(),
                ),
                ids: None,
            },
        };

    match runtime.block_on(extract_and_store(store, llm_client, req)) {
        Ok(ids) => ApiResponse {
            success: true,
            message: None,
            ids: Some(ids),
        },
        Err(e) => ApiResponse {
            success: false,
            message: Some(e),
            ids: None,
        },
    }
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

fn build_llm_client_from_env() -> Result<Arc<dyn LlmClient>, String> {
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

fn env_var(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

fn is_authorized_extension_request(
    request: &tiny_http::Request,
    allowed_origin: Option<&str>,
) -> bool {
    if allowed_origin.is_none() {
        return false;
    }

    matches!(
        get_client_header(request).as_deref(),
        Some(CLIENT_HEADER_VALUE)
    )
}

fn is_allowed_extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://")
}

fn get_origin_header(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Origin"))
        .map(|header| header.value.as_str().to_string())
}

fn get_client_header(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(CLIENT_HEADER_NAME))
        .map(|header| header.value.as_str().to_string())
}

fn empty_response(
    status_code: u16,
    allowed_origin: Option<&str>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn json_response(
    status_code: u16,
    payload: &ApiResponse,
    allowed_origin: Option<&str>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn add_common_headers(
    response: &mut Response<std::io::Cursor<Vec<u8>>>,
    allowed_origin: Option<&str>,
) {
    response.add_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    if let Some(origin) = allowed_origin {
        response.add_header(Header::from_bytes("Access-Control-Allow-Origin", origin).unwrap());
        response.add_header(Header::from_bytes("Vary", "Origin").unwrap());
        response.add_header(
            Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
        );
        response.add_header(
            Header::from_bytes(
                "Access-Control-Allow-Headers",
                format!("Content-Type, {}", CLIENT_HEADER_NAME),
            )
            .unwrap(),
        );
    }
}
