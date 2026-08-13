use super::extract::{self, IngestRequest};
use super::json::parse_json_body;
use refine_core::infra::{
    normalize_conversation_input, validate_contract_version, CreateConversationRequest, ItemDto,
    LlmClient, CONTRACT_VERSION, CONTRACT_VERSION_HEADER,
};
use refine_core::knowledge::{DocumentRepository, ItemRepository};
use serde_json::json;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response};
use tokio::runtime::Runtime;

const CLIENT_HEADER_NAME: &str = "X-Refine-Client";
const CLIENT_HEADER_VALUE: &str = "extension";

pub(super) fn handle_request(
    request: &mut tiny_http::Request,
    store: &Arc<dyn ItemRepository>,
    doc_store: &Arc<dyn DocumentRepository>,
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
            json!({"success": false, "message": "Forbidden origin"}),
            None,
        );
    }

    if let Some(response) = incompatible_contract_response(request, allowed_origin.as_deref()) {
        return response;
    }

    let (path, query) = split_path_and_query(request.url());
    let method = request.method().clone();

    match (method, path) {
        (Method::Get, "/health") => json_response(
            200,
            json!({
                "success": true,
                "message": "Refine local API is running",
                "contract_version": CONTRACT_VERSION
            }),
            allowed_origin.as_deref(),
        ),
        (Method::Post, "/v1/conversations") => {
            if !is_authorized_extension_request(request, allowed_origin.as_deref()) {
                return unauthorized_response(allowed_origin.as_deref());
            }
            handle_create_conversation(
                request,
                doc_store,
                runtime,
                llm_client,
                allowed_origin.as_deref(),
            )
        }
        (Method::Get, "/v1/items") => {
            if !is_authorized_extension_request(request, allowed_origin.as_deref()) {
                return unauthorized_response(allowed_origin.as_deref());
            }
            handle_list_items(store, runtime, query, allowed_origin.as_deref())
        }
        _ => json_response(
            404,
            json!({"success": false, "message": "Not found"}),
            allowed_origin.as_deref(),
        ),
    }
}

fn handle_create_conversation(
    request: &mut tiny_http::Request,
    doc_store: &Arc<dyn DocumentRepository>,
    runtime: &Runtime,
    llm_client: Option<&Arc<dyn LlmClient>>,
    allowed_origin: Option<&str>,
) -> Response<Cursor<Vec<u8>>> {
    let payload = match parse_json_body::<CreateConversationRequest>(request) {
        Ok(payload) => payload,
        Err(err) => {
            return json_response(
                400,
                json!({"success": false, "message": err}),
                allowed_origin,
            );
        }
    };

    let CreateConversationRequest {
        content,
        url,
        source,
        title,
        idempotency_key,
        ..
    } = payload;

    let normalized =
        match normalize_conversation_input(content, url, source, title, idempotency_key) {
            Ok(value) => value,
            Err(err) => {
                return json_response(
                    400,
                    json!({"success": false, "message": err}),
                    allowed_origin,
                );
            }
        };

    let extract_result = extract::ingest_conversation(
        doc_store,
        runtime,
        llm_client,
        IngestRequest {
            content: normalized.content,
            url: normalized.url,
            source: normalized.source,
            title: normalized.title,
        },
    );

    match extract_result {
        Ok(_) => {
            let conversation_id = generate_conversation_id();
            json_response(
                200,
                json!({
                    "success": true,
                    "conversation_id": conversation_id,
                    "status": "queued"
                }),
                allowed_origin,
            )
        }
        Err(err) => json_response(
            400,
            json!({
                "success": false,
                "message": err
            }),
            allowed_origin,
        ),
    }
}

fn handle_list_items(
    store: &Arc<dyn ItemRepository>,
    runtime: &Runtime,
    query: Option<&str>,
    allowed_origin: Option<&str>,
) -> Response<Cursor<Vec<u8>>> {
    let cursor = parse_usize_query(query, "cursor", 0);
    let limit = parse_usize_query(query, "limit", 20).clamp(1, 100);

    let total = match runtime.block_on(store.count_items(None)) {
        Ok(total) => total,
        Err(err) => {
            return json_response(
                500,
                json!({"success": false, "message": err.to_string()}),
                allowed_origin,
            )
        }
    };
    let items = match runtime.block_on(store.find_recent(None, cursor, limit)) {
        Ok(items) => items,
        Err(err) => {
            return json_response(
                500,
                json!({"success": false, "message": err.to_string()}),
                allowed_origin,
            )
        }
    };

    let data = items.iter().map(ItemDto::from).collect::<Vec<_>>();
    let next_cursor = if cursor + data.len() < total {
        Some(cursor + data.len())
    } else {
        None
    };

    json_response(
        200,
        json!({
            "success": true,
            "items": data,
            "total": total,
            "next_cursor": next_cursor
        }),
        allowed_origin,
    )
}

fn split_path_and_query(url: &str) -> (&str, Option<&str>) {
    if let Some((path, query)) = url.split_once('?') {
        (path, Some(query))
    } else {
        (url, None)
    }
}

fn parse_usize_query(query: Option<&str>, key: &str, default: usize) -> usize {
    find_query_value(query, key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn find_query_value<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    let query = query?;
    query
        .split('&')
        .filter_map(|entry| entry.split_once('='))
        .find_map(|(k, v)| if k == key { Some(v) } else { None })
}

fn generate_conversation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("local-{}", nanos)
}

fn unauthorized_response(allowed_origin: Option<&str>) -> Response<Cursor<Vec<u8>>> {
    json_response(
        403,
        json!({"success": false, "message": "Unauthorized extension request"}),
        allowed_origin,
    )
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
    get_header_value(request, "Origin")
}

fn get_client_header(request: &tiny_http::Request) -> Option<String> {
    get_header_value(request, CLIENT_HEADER_NAME)
}

fn get_contract_version_header(request: &tiny_http::Request) -> Option<String> {
    get_header_value(request, CONTRACT_VERSION_HEADER)
}

fn get_header_value(request: &tiny_http::Request, header_name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.to_string().eq_ignore_ascii_case(header_name))
        .map(|header| header.value.as_str().to_string())
}

fn incompatible_contract_response(
    request: &tiny_http::Request,
    allowed_origin: Option<&str>,
) -> Option<Response<Cursor<Vec<u8>>>> {
    let raw = get_contract_version_header(request)?;
    match validate_contract_version(Some(raw.as_str()), CONTRACT_VERSION) {
        Ok(()) => None,
        Err(message) => Some(json_response(
            426,
            json!({
                "success": false,
                "message": message
            }),
            allowed_origin,
        )),
    }
}

fn empty_response(status_code: u16, allowed_origin: Option<&str>) -> Response<Cursor<Vec<u8>>> {
    let mut response = Response::from_data(Vec::new()).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn json_response(
    status_code: u16,
    payload: serde_json::Value,
    allowed_origin: Option<&str>,
) -> Response<Cursor<Vec<u8>>> {
    let body = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::from_data(body).with_status_code(status_code);
    add_common_headers(&mut response, allowed_origin);
    response
}

fn add_common_headers(response: &mut Response<Cursor<Vec<u8>>>, allowed_origin: Option<&str>) {
    response.add_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    response.add_header(Header::from_bytes(CONTRACT_VERSION_HEADER, CONTRACT_VERSION).unwrap());

    if let Some(origin) = allowed_origin {
        response.add_header(Header::from_bytes("Access-Control-Allow-Origin", origin).unwrap());
        response.add_header(Header::from_bytes("Vary", "Origin").unwrap());
        response.add_header(
            Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
        );
        response.add_header(
            Header::from_bytes(
                "Access-Control-Allow-Headers",
                format!(
                    "Content-Type, Authorization, {}, {}",
                    CLIENT_HEADER_NAME, CONTRACT_VERSION_HEADER
                ),
            )
            .unwrap(),
        );
        response.add_header(
            Header::from_bytes(
                "Access-Control-Expose-Headers",
                format!("Content-Type, {}", CONTRACT_VERSION_HEADER),
            )
            .unwrap(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::CONTRACT_VERSION;
    use refine_core::infra::validate_contract_version;

    #[test]
    fn client_contract_compatibility_uses_major_version() {
        assert!(validate_contract_version(Some(""), CONTRACT_VERSION).is_ok());
        assert!(validate_contract_version(Some("1.0"), CONTRACT_VERSION).is_ok());
        assert!(validate_contract_version(Some("1.9.9"), CONTRACT_VERSION).is_ok());
        assert!(validate_contract_version(Some("2.0"), CONTRACT_VERSION).is_err());
    }
}
