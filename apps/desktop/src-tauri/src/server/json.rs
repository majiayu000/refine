use serde::de::DeserializeOwned;

pub(super) fn parse_json_body<T: DeserializeOwned>(
    request: &mut tiny_http::Request,
) -> Result<T, String> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|_| "Failed to read request body".to_string())?;
    serde_json::from_str(&body).map_err(|err| format!("Invalid JSON: {}", err))
}
