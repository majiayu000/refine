use axum::http::HeaderMap;

const UNAUTHORIZED_MESSAGE: &str =
    "Unauthorized: provide Authorization: Bearer <token> and ensure REFINE_API_TOKEN matches.";

pub fn authorize_user(headers: &HeaderMap, expected_token: Option<&str>) -> Result<String, String> {
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

        if provided != token {
            return Err(UNAUTHORIZED_MESSAGE.to_string());
        }

        return Ok("token-user".to_string());
    }

    Ok("dev-user".to_string())
}

#[cfg(test)]
mod tests {
    use super::authorize_user;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn authorize_rejects_missing_token_when_required() {
        let headers = HeaderMap::new();
        let result = authorize_user(&headers, Some("secret-token"));
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

        let result = authorize_user(&headers, Some("secret-token"));
        assert!(result.is_ok());
    }
}
