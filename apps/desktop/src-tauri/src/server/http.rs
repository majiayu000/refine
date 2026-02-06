use super::extract;
use refine_core::infra::{LlmClient, SqliteStore};
use serde::Serialize;
use std::io::Cursor;
use std::sync::Arc;
use tiny_http::{Header, Method, Response};
use tokio::runtime::Runtime;

const CLIENT_HEADER_NAME: &str = "X-Refine-Client";
const CLIENT_HEADER_VALUE: &str = "extension";

#[derive(Debug, Serialize)]
struct ApiResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ids: Option<Vec<String>>,
}

pub(super) fn handle_request(
    request: &mut tiny_http::Request,
    store: &Arc<SqliteStore>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
) -> Response<Cursor<Vec<u8>>> {
    let origin = get_origin_header(request);
    let allowed_origin = origin
        .as_deref()
        .filter(|value| is_allowed_extension_origin(value))
        .map(str::to_string);

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
        (Method::Get, "/health") => json_response(
            200,
            &ApiResponse {
                success: true,
                message: Some("Refine is running".to_string()),
                ids: None,
            },
            allowed_origin.as_deref(),
        ),
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

            match extract::handle_extract(request, store, runtime, llm_client) {
                Ok(ids) => json_response(
                    200,
                    &ApiResponse {
                        success: true,
                        message: None,
                        ids: Some(ids),
                    },
                    allowed_origin.as_deref(),
                ),
                Err(err) => json_response(
                    400,
                    &ApiResponse {
                        success: false,
                        message: Some(err),
                        ids: None,
                    },
                    allowed_origin.as_deref(),
                ),
            }
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

fn is_authorized_extension_request(request: &tiny_http::Request, allowed_origin: Option<&str>) -> bool {
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

fn empty_response(status_code: u16, allowed_origin: Option<&str>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn json_response(
    status_code: u16,
    payload: &ApiResponse,
    allowed_origin: Option<&str>,
) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn add_common_headers(response: &mut Response<Cursor<Vec<u8>>>, allowed_origin: Option<&str>) {
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
