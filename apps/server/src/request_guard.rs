use axum::extract::{FromRef, FromRequestParts};
use axum::http::{request::Parts, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use crate::auth::authorize_user;
use crate::state::AppState;

pub const SERVER_CONTRACT_VERSION: &str = "1.0";
pub const CONTRACT_VERSION_HEADER: &str = "x-refine-contract-version";

pub struct AuthenticatedUser(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        validate_client_contract(&parts.headers)?;
        let state = Arc::<AppState>::from_ref(state);
        let user_id = authorize_user(&parts.headers, state.api_token.as_deref())
            .map_err(|err| err_response(StatusCode::UNAUTHORIZED, &err))?;
        Ok(Self(user_id))
    }
}

pub fn with_contract_header(mut response: Response) -> Response {
    if let Ok(value) = HeaderValue::from_str(SERVER_CONTRACT_VERSION) {
        response.headers_mut().insert(CONTRACT_VERSION_HEADER, value);
    }
    response
}

pub fn validate_client_contract(headers: &HeaderMap) -> Result<(), Response> {
    let Some(raw) = headers.get(CONTRACT_VERSION_HEADER) else {
        return Ok(());
    };
    let raw = raw
        .to_str()
        .map_err(|_| err_response(StatusCode::BAD_REQUEST, "invalid contract version header"))?;
    if raw.trim().is_empty() {
        return Ok(());
    }
    if is_contract_compatible(raw) {
        return Ok(());
    }
    Err(err_response(
        StatusCode::UPGRADE_REQUIRED,
        &format!(
            "Client contract version {} is incompatible with server {}",
            raw, SERVER_CONTRACT_VERSION
        ),
    ))
}

fn is_contract_compatible(client_version: &str) -> bool {
    let client_major = normalize_contract_major(client_version);
    let server_major = normalize_contract_major(SERVER_CONTRACT_VERSION);
    !client_major.is_empty() && client_major == server_major
}

fn normalize_contract_major(version: &str) -> &str {
    let version = version.trim();
    version.split('.').next().unwrap_or(version)
}

fn err_response(status: StatusCode, message: &str) -> Response {
    with_contract_header(
        (
            status,
            Json(json!({
                "success": false,
                "message": message
            })),
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        is_contract_compatible, normalize_contract_major, validate_client_contract,
        CONTRACT_VERSION_HEADER,
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode};

    #[test]
    fn normalize_contract_major_uses_first_segment() {
        assert_eq!(normalize_contract_major("1.2.3"), "1");
        assert_eq!(normalize_contract_major(" 2 "), "2");
    }

    #[test]
    fn contract_compatibility_checks_major_version() {
        assert!(is_contract_compatible("1.0"));
        assert!(is_contract_compatible("1.9.9"));
        assert!(!is_contract_compatible("2.0"));
    }

    #[test]
    fn validate_contract_allows_missing_header() {
        let headers = HeaderMap::new();
        assert!(validate_client_contract(&headers).is_ok());
    }

    #[test]
    fn validate_contract_rejects_incompatible_version() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTRACT_VERSION_HEADER, HeaderValue::from_static("2.0"));
        let response = validate_client_contract(&headers).expect_err("expected mismatch error");
        assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
    }
}
