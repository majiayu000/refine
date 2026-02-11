use super::extract::{self, IngestRequest};
use refine_core::infra::{
    is_contract_compatible, LlmClient, CONTRACT_VERSION, CONTRACT_VERSION_HEADER,
};
use refine_core::knowledge::{Item, ItemRepository};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Method, Response};
use tokio::runtime::Runtime;

const CLIENT_HEADER_NAME: &str = "X-Refine-Client";
const CLIENT_HEADER_VALUE: &str = "extension";

#[derive(Debug, Deserialize)]
struct CreateConversationRequest {
    content: Option<String>,
    url: Option<String>,
    source: Option<String>,
    title: Option<String>,
    idempotency_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct ItemDto {
    id: String,
    item_type: String,
    title: String,
    summary: String,
    content: String,
    tags: Vec<String>,
    created_at: String,
}

impl From<&Item> for ItemDto {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id().to_string(),
            item_type: item.item_type().as_str().to_string(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            tags: item
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_string())
                .collect(),
            created_at: item.created_at().to_rfc3339(),
        }
    }
}

pub(super) fn handle_request(
    request: &mut tiny_http::Request,
    store: &Arc<dyn ItemRepository>,
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
        (Method::Post, "/extract") => {
            if !is_authorized_extension_request(request, allowed_origin.as_deref()) {
                return unauthorized_response(allowed_origin.as_deref());
            }

            match extract::handle_extract(request, store, runtime, llm_client) {
                Ok(ids) => json_response(
                    200,
                    json!({"success": true, "ids": ids}),
                    allowed_origin.as_deref(),
                ),
                Err(err) => json_response(
                    400,
                    json!({"success": false, "message": err}),
                    allowed_origin.as_deref(),
                ),
            }
        }
        (Method::Post, "/v1/conversations") => {
            if !is_authorized_extension_request(request, allowed_origin.as_deref()) {
                return unauthorized_response(allowed_origin.as_deref());
            }
            handle_create_conversation(
                request,
                store,
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
    store: &Arc<dyn ItemRepository>,
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
            )
        }
    };

    let content = match payload.content.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => value,
        _ => {
            return json_response(
                400,
                json!({"success": false, "message": "Missing required field: content"}),
                allowed_origin,
            )
        }
    };
    let url = match payload.url.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => value,
        _ => {
            return json_response(
                400,
                json!({"success": false, "message": "Missing required field: url"}),
                allowed_origin,
            )
        }
    };
    let source = match payload.source.map(|value| value.trim().to_string()) {
        Some(value) if !value.is_empty() => value,
        _ => {
            return json_response(
                400,
                json!({"success": false, "message": "Missing required field: source"}),
                allowed_origin,
            )
        }
    };
    let idempotency_key = match payload
        .idempotency_key
        .map(|value| value.trim().to_string())
    {
        Some(value) if !value.is_empty() => value,
        _ => {
            return json_response(
                400,
                json!({"success": false, "message": "Missing required field: idempotency_key"}),
                allowed_origin,
            )
        }
    };

    if let Some(conversation_id) = find_idempotency_hit(&idempotency_key) {
        return json_response(
            200,
            json!({
                "success": true,
                "conversation_id": conversation_id,
                "status": "queued",
                "deduplicated": true
            }),
            allowed_origin,
        );
    }

    let extract_result = extract::ingest_conversation(
        store,
        runtime,
        llm_client,
        IngestRequest {
            content,
            url,
            source,
            title: payload.title,
        },
    );

    match extract_result {
        Ok(_) => {
            let conversation_id = generate_conversation_id();
            remember_idempotency(idempotency_key, conversation_id.clone());
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

fn parse_json_body<T: for<'de> Deserialize<'de>>(
    request: &mut tiny_http::Request,
) -> Result<T, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|_| "Failed to read request body".to_string())?;
    serde_json::from_str(&body).map_err(|err| format!("Invalid JSON: {}", err))
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

fn idempotency_index() -> &'static Mutex<HashMap<String, String>> {
    static INDEX: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    INDEX.get_or_init(|| Mutex::new(HashMap::new()))
}

fn find_idempotency_hit(idempotency_key: &str) -> Option<String> {
    let guard = idempotency_index().lock().ok()?;
    guard.get(idempotency_key).cloned()
}

fn remember_idempotency(idempotency_key: String, conversation_id: String) {
    if let Ok(mut guard) = idempotency_index().lock() {
        guard.insert(idempotency_key, conversation_id);
    }
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

fn get_contract_version_header(request: &tiny_http::Request) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(CONTRACT_VERSION_HEADER))
        .map(|header| header.value.as_str().to_string())
}

fn is_client_contract_compatible(raw_version: &str) -> bool {
    let version = raw_version.trim();
    version.is_empty() || is_contract_compatible(version, CONTRACT_VERSION)
}

fn incompatible_contract_response(
    request: &tiny_http::Request,
    allowed_origin: Option<&str>,
) -> Option<Response<Cursor<Vec<u8>>>> {
    let raw = get_contract_version_header(request)?;
    if is_client_contract_compatible(&raw) {
        return None;
    }

    Some(json_response(
        426,
        json!({
            "success": false,
            "message": format!(
                "Client contract version {} is incompatible with server {}",
                raw.trim(),
                CONTRACT_VERSION
            )
        }),
        allowed_origin,
    ))
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
    use super::is_client_contract_compatible;

    #[test]
    fn client_contract_compatibility_uses_major_version() {
        assert!(is_client_contract_compatible(""));
        assert!(is_client_contract_compatible("1.0"));
        assert!(is_client_contract_compatible("1.9.9"));
        assert!(!is_client_contract_compatible("2.0"));
    }
}
