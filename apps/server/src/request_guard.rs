use async_trait::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::{request::Parts, HeaderMap, StatusCode};
use axum::response::Response;
use refine_core::infra::validate_contract_version;
use std::sync::Arc;

use crate::api_response::error_message;
use crate::auth::authorize_user;
use crate::state::AppState;

pub use crate::api_response::{CONTRACT_VERSION_HEADER, SERVER_CONTRACT_VERSION};

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
        let user_id = authorize_user(&parts.headers, state.api_token.as_deref(), state.dev_anon)
            .map_err(|err| error_message(StatusCode::UNAUTHORIZED, &err))?;
        Ok(Self(user_id))
    }
}

#[allow(clippy::result_large_err)] // Response is the idiomatic Axum rejection type
pub fn validate_client_contract(headers: &HeaderMap) -> Result<(), Response> {
    let raw = headers
        .get(CONTRACT_VERSION_HEADER)
        .map(|value| {
            value.to_str().map_err(|_| {
                error_message(StatusCode::BAD_REQUEST, "invalid contract version header")
            })
        })
        .transpose()?;
    validate_contract_version(raw, SERVER_CONTRACT_VERSION)
        .map_err(|message| error_message(StatusCode::UPGRADE_REQUIRED, &message))
}

#[cfg(test)]
mod tests {
    use super::{validate_client_contract, CONTRACT_VERSION_HEADER};
    use axum::http::{HeaderMap, HeaderValue, StatusCode};
    use refine_core::infra::{is_contract_compatible, normalize_contract_major};

    #[test]
    fn normalize_contract_major_uses_first_segment() {
        assert_eq!(normalize_contract_major("1.2.3"), "1");
        assert_eq!(normalize_contract_major(" 2 "), "2");
    }

    #[test]
    fn contract_compatibility_checks_major_version() {
        assert!(is_contract_compatible("1.0", "1.2"));
        assert!(is_contract_compatible("1.9.9", "1"));
        assert!(!is_contract_compatible("2.0", "1.0"));
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
