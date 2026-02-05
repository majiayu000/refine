//! 本地 HTTP API 服务器
//!
//! 提供 HTTP API 供浏览器扩展调用

use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemRepository, Source};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::Arc;
use tiny_http::{Header, Method, Response, Server};

const SERVER_PORT: u16 = 19527;

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
    id: Option<String>,
}

/// 启动 HTTP 服务器
pub fn start_server(store: Arc<SqliteStore>) {
    std::thread::spawn(move || {
        let server = match Server::http(format!("127.0.0.1:{}", SERVER_PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to start HTTP server: {}", e);
                return;
            }
        };

        println!("Refine API: http://localhost:{}", SERVER_PORT);

        for mut request in server.incoming_requests() {
            let response = handle_request(&mut request, &store);
            let _ = request.respond(response);
        }
    });
}

fn handle_request(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    // CORS headers
    let cors_headers = vec![
        Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
        Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap(),
        Header::from_bytes("Content-Type", "application/json").unwrap(),
    ];

    // Handle preflight
    if request.method() == &Method::Options {
        let mut response = Response::from_string("").with_status_code(204);
        for header in cors_headers {
            response.add_header(header);
        }
        return response;
    }

    let path = request.url().to_string();
    let method = request.method().clone();

    let result = match (method, path.as_str()) {
        (Method::Get, "/health") => handle_health(),
        (Method::Post, "/extract") => handle_extract(request, store),
        _ => ApiResponse {
            success: false,
            message: Some("Not found".to_string()),
            id: None,
        },
    };

    let json = serde_json::to_vec(&result).unwrap_or_default();
    let mut response = Response::from_data(json);
    for header in cors_headers {
        response.add_header(header);
    }
    response
}

fn handle_health() -> ApiResponse {
    ApiResponse {
        success: true,
        message: Some("Refine is running".to_string()),
        id: None,
    }
}

fn handle_extract(request: &mut tiny_http::Request, store: &Arc<SqliteStore>) -> ApiResponse {
    // 读取请求体
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        return ApiResponse {
            success: false,
            message: Some("Failed to read request body".to_string()),
            id: None,
        };
    }

    // 解析 JSON
    let req: ExtractRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return ApiResponse {
                success: false,
                message: Some(format!("Invalid JSON: {}", e)),
                id: None,
            };
        }
    };

    // 创建知识条目
    let title = format!("对话 - {}", req.source);
    let summary = req.content.chars().take(200).collect::<String>();
    let item = Item::new_knowledge(&title, &summary)
        .with_source(Source::new(&req.source).with_url(&req.url));

    let id = item.id().to_string();

    // 保存 (同步执行)
    let rt = tokio::runtime::Runtime::new().unwrap();
    match rt.block_on(store.save(&item)) {
        Ok(_) => ApiResponse {
            success: true,
            message: None,
            id: Some(id),
        },
        Err(e) => ApiResponse {
            success: false,
            message: Some(e.to_string()),
            id: None,
        },
    }
}
