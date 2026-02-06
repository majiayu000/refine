use axum::http::HeaderMap;

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
            return Err("Unauthorized".to_string());
        }

        return Ok("token-user".to_string());
    }

    Ok("dev-user".to_string())
}
