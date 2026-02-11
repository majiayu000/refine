use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
pub use refine_core::infra::CONTRACT_VERSION_HEADER;
use refine_core::infra::CONTRACT_VERSION;
use serde::Serialize;
use serde_json::{json, Map, Value};

pub const SERVER_CONTRACT_VERSION: &str = CONTRACT_VERSION;

pub fn with_contract_header(mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(SERVER_CONTRACT_VERSION) {
        response
            .headers_mut()
            .insert(CONTRACT_VERSION_HEADER, value);
    }
    response
}

pub fn ok(payload: Value) -> Response {
    let mut body = Map::new();
    body.insert("success".to_string(), Value::Bool(true));
    if let Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    with_contract_header((StatusCode::OK, Json(Value::Object(body))).into_response())
}

pub fn ok_serializable<T>(payload: T) -> Response
where
    T: Serialize,
{
    match serde_json::to_value(payload) {
        Ok(value) => ok(value),
        Err(err) => error_message(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

pub fn error_message(status: StatusCode, message: &str) -> Response {
    error_payload(
        status,
        json!({
            "message": message
        }),
    )
}

pub fn error_payload(status: StatusCode, payload: Value) -> Response {
    let mut body = Map::new();
    body.insert("success".to_string(), Value::Bool(false));
    if let Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    with_contract_header((status, Json(Value::Object(body))).into_response())
}

#[cfg(test)]
mod tests {
    use super::{error_message, ok, CONTRACT_VERSION_HEADER, SERVER_CONTRACT_VERSION};
    use axum::body::to_bytes;
    use axum::http::StatusCode;
    use serde_json::json;

    #[tokio::test]
    async fn ok_response_contains_success_and_contract_header() {
        let response = ok(json!({ "id": "x" }));
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTRACT_VERSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(SERVER_CONTRACT_VERSION)
        );

        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("response body should be json");
        assert_eq!(body.get("success"), Some(&json!(true)));
        assert_eq!(body.get("id"), Some(&json!("x")));
    }

    #[tokio::test]
    async fn error_response_contains_success_false_and_message() {
        let response = error_message(StatusCode::BAD_REQUEST, "bad request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("response body should be json");
        assert_eq!(body.get("success"), Some(&json!(false)));
        assert_eq!(body.get("message"), Some(&json!("bad request")));
    }
}
