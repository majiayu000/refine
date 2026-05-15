use axum::http::HeaderMap;
use subtle::ConstantTimeEq;

const UNAUTHORIZED_MESSAGE: &str =
    "Unauthorized: provide Authorization: Bearer <token> and ensure REFINE_API_TOKEN matches.";

const ANON_DISABLED_MESSAGE: &str = "Unauthorized: REFINE_API_TOKEN is not set. \
     Set REFINE_DEV_ANON=1 to allow unauthenticated local access in development.";

/// Authenticate the request. Returns a user identifier on success.
///
/// - If `expected_token` is set: the request must carry a matching Bearer token.
/// - If `expected_token` is absent and `dev_anon` is true: returns "dev-user" (dev opt-in).
/// - If `expected_token` is absent and `dev_anon` is false: returns an error (secure default).
pub fn authorize_user(
    headers: &HeaderMap,
    expected_token: Option<&str>,
    dev_anon: bool,
) -> Result<String, String> {
    if let Some(token) = expected_token.filter(|v| !v.trim().is_empty()) {
        let authorization = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .trim();

        let provided = authorization
            .strip_prefix("Bearer ")
            .or_else(|| authorization.strip_prefix("bearer "))
            .unwrap_or_default()
            .trim();

        // Constant-time comparison to avoid byte-by-byte timing leak (HI-7).
        // `ct_eq` short-circuits to false on length mismatch in constant time.
        if provided.as_bytes().ct_eq(token.as_bytes()).unwrap_u8() != 1 {
            return Err(UNAUTHORIZED_MESSAGE.to_string());
        }

        return Ok("token-user".to_string());
    }

    if dev_anon {
        return Ok("dev-user".to_string());
    }

    Err(ANON_DISABLED_MESSAGE.to_string())
}

#[cfg(test)]
mod tests {
    use super::authorize_user;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn authorize_rejects_missing_token_when_required() {
        let headers = HeaderMap::new();
        let result = authorize_user(&headers, Some("secret-token"), false);
        assert!(result.is_err());
        assert!(result
            .err()
            .unwrap_or_default()
            .contains("Authorization: Bearer"));
    }

    #[test]
    fn authorize_accepts_matching_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-token"),
        );

        let result = authorize_user(&headers, Some("secret-token"), false);
        assert!(result.is_ok());
    }

    #[test]
    fn authorize_rejects_anonymous_when_dev_anon_disabled() {
        let headers = HeaderMap::new();
        // No token configured, dev_anon opt-in is off — must return 401
        let result = authorize_user(&headers, None, false);
        assert!(result.is_err(), "anonymous request must be rejected");
        assert!(
            result.err().unwrap_or_default().contains("REFINE_DEV_ANON"),
            "error message must mention REFINE_DEV_ANON"
        );
    }

    #[test]
    fn authorize_rejects_token_with_wrong_length() {
        // Constant-time comparison must still reject length-mismatched tokens (HI-7).
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));

        let result = authorize_user(&headers, Some("secret-token"), false);
        assert!(result.is_err(), "shorter token must be rejected");
    }

    #[test]
    fn authorize_rejects_token_with_one_byte_differ() {
        // Single-byte mismatch must still produce an unauthorized error (HI-7).
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-tokeX"),
        );

        let result = authorize_user(&headers, Some("secret-token"), false);
        assert!(result.is_err(), "differing-byte token must be rejected");
    }

    #[test]
    fn authorize_accepts_anonymous_when_dev_anon_enabled() {
        let headers = HeaderMap::new();
        let result = authorize_user(&headers, None, true);
        assert_eq!(result.unwrap(), "dev-user");
    }
}
