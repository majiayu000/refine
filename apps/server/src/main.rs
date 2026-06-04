mod api_response;
mod application;
mod auth;
mod extraction;
mod handlers;
mod models;
mod request_guard;
mod state;
mod vector_search;

use axum::http::{header, HeaderValue, Method};
use axum::routing::{delete, get, post};
use axum::Router;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

const DEFAULT_BIND_HOST: &str = "127.0.0.1";
const DEFAULT_SERVER_PORT: u16 = 21567;
/// Candidate ports when the preferred port is occupied.
/// The extension probes the same list for discovery.
const FALLBACK_PORTS: &[u16] = &[21567, 21568, 21569, 21570];
const MAX_FALLBACK_ATTEMPTS: usize = FALLBACK_PORTS.len();

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter("refine_server=info,refine_core=info")
        .init();

    let state = match AppState::build().await {
        Ok(state) => Arc::new(state),
        Err(err) => {
            eprintln!("Failed to initialize app state: {}", err);
            std::process::exit(1);
        }
    };

    if state.llm_client.is_none() {
        println!(
            "LLM is not configured, extraction requests will fail in strict mode. \
             Set REFINE_ANTHROPIC_API_KEY or REFINE_OPENAI_API_KEY to enable extraction."
        );
    }
    let app = build_app(state.clone(), trusted_origins_from_env());

    let preferred_port = std::env::var("REFINE_SERVER_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_SERVER_PORT);
    let host =
        std::env::var("REFINE_SERVER_HOST").unwrap_or_else(|_| DEFAULT_BIND_HOST.to_string());

    let addr = resolve_bind_addr(&host, preferred_port).await;
    if requires_api_token_for_bind(&addr) && state.api_token.is_none() {
        eprintln!(
            "REFINE_API_TOKEN is required when binding to non-loopback address: {}",
            addr.ip()
        );
        std::process::exit(1);
    }

    println!("Refine cloud API (Rust) listening on http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind {}: {}", addr, e);
            std::process::exit(1);
        }
    };
    axum::serve(listener, app).await.expect("server exited");
}

/// Try the preferred port, then fallback candidates.
/// - If another refine-server is already running on any candidate → exit(0).
/// - If a non-refine process holds the port → skip to next candidate.
/// - If all candidates exhausted → exit(1) with diagnostics.
async fn resolve_bind_addr(host: &str, preferred: u16) -> SocketAddr {
    use std::net::TcpStream;
    use std::time::Duration;

    let timeout = Duration::from_millis(200);

    // Build candidate list: preferred first, then fallbacks (deduped)
    let mut candidates = vec![preferred];
    for &p in FALLBACK_PORTS {
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    }
    candidates.truncate(MAX_FALLBACK_ATTEMPTS);

    for &port in &candidates {
        let addr = parse_bind_addr(host, port);

        // Can we connect? If not, the port is free.
        if TcpStream::connect_timeout(&addr, timeout).is_err() {
            if port != preferred {
                eprintln!(
                    "Port {} was occupied, using fallback port {}.",
                    preferred, port
                );
            }
            return addr;
        }

        // Port is occupied — check if it's us
        if is_refine_server(&addr).await {
            eprintln!(
                "Another refine-server is already running on {}. Exiting.",
                addr
            );
            std::process::exit(0);
        }

        // Not us — log and try next
        eprintln!(
            "Port {} is occupied by another process, trying next...",
            port
        );
    }

    eprintln!(
        "All candidate ports {:?} are occupied. \
         Set REFINE_SERVER_PORT to specify a free port.",
        candidates
    );
    std::process::exit(1);
}

async fn is_refine_server(addr: &SocketAddr) -> bool {
    let url = format!("http://{}/health", addr);
    match reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp
            .text()
            .await
            .is_ok_and(|body| body.contains("Refine cloud API")),
        Err(_) => false,
    }
}

/// Build the CORS allowed-origin list from `REFINE_TRUSTED_ORIGINS` (comma-separated).
/// Returns an empty list when the variable is unset, blocking all cross-origin requests.
fn trusted_origins_from_env() -> AllowOrigin {
    let origins: Vec<HeaderValue> = std::env::var("REFINE_TRUSTED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<HeaderValue>().ok())
        .collect();
    AllowOrigin::list(origins)
}

fn build_cors(allowed_origins: AllowOrigin) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-refine-client"),
            header::HeaderName::from_static(api_response::CONTRACT_VERSION_HEADER),
        ])
        .expose_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static(api_response::CONTRACT_VERSION_HEADER),
        ])
        .max_age(std::time::Duration::from_secs(60 * 60))
}

fn build_app(state: Arc<AppState>, allowed_origins: AllowOrigin) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard_page))
        .route("/dashboard", get(handlers::dashboard_page))
        .route("/health", get(handlers::health))
        .route(
            "/v1/conversations",
            get(handlers::list_conversations).post(handlers::create_conversation),
        )
        .route("/v1/events", post(handlers::create_event))
        .route("/v1/events/summary", get(handlers::get_event_summary))
        .route("/v1/extraction-jobs", post(handlers::create_extraction_job))
        .route(
            "/v1/extraction-jobs/:job_id",
            get(handlers::get_extraction_job),
        )
        .route("/v1/documents", get(handlers::list_documents))
        .route("/v1/documents/:doc_id", get(handlers::get_document))
        .route("/v1/items", get(handlers::list_items))
        .route("/v1/items/:item_id", delete(handlers::delete_item))
        .route("/v1/quota", get(handlers::get_quota))
        .route("/v1/search", get(handlers::search_items))
        .route("/v1/recommendations", get(handlers::recommend_items))
        .layer(build_cors(allowed_origins))
        .with_state(state)
}

fn parse_bind_addr(host: &str, port: u16) -> SocketAddr {
    format!("{}:{}", host, port)
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)))
}

fn requires_api_token_for_bind(addr: &SocketAddr) -> bool {
    !addr.ip().is_loopback()
}

#[cfg(test)]
mod tests {
    use super::{build_app, parse_bind_addr, requires_api_token_for_bind, DEFAULT_SERVER_PORT};
    use crate::state::{AppState, AuthConfig};
    use axum::body::Body;
    use axum::http::{header, HeaderValue, Method, Request, StatusCode};
    use axum::Router;
    use serde_json::json;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;
    use tower_http::cors::AllowOrigin;

    #[test]
    fn parse_bind_addr_falls_back_to_loopback_for_invalid_host() {
        let addr = parse_bind_addr("localhost", DEFAULT_SERVER_PORT);
        assert_eq!(
            addr,
            SocketAddr::from(([127, 0, 0, 1], DEFAULT_SERVER_PORT))
        );
    }

    #[test]
    fn non_loopback_bindings_require_api_token() {
        assert!(requires_api_token_for_bind(&SocketAddr::from((
            [0, 0, 0, 0],
            DEFAULT_SERVER_PORT
        ))));
        assert!(requires_api_token_for_bind(&SocketAddr::from((
            [192, 168, 1, 8],
            DEFAULT_SERVER_PORT
        ))));
        assert!(!requires_api_token_for_bind(&SocketAddr::from((
            [127, 0, 0, 1],
            DEFAULT_SERVER_PORT
        ))));
        assert!(!requires_api_token_for_bind(&SocketAddr::from((
            [0, 0, 0, 0, 0, 0, 0, 1],
            DEFAULT_SERVER_PORT
        ))));
    }

    #[tokio::test]
    async fn health_route_remains_public() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: None,
                dev_anon: false,
            },
            &[],
        )
        .await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch health request");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_route_rejects_anonymous_by_default() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: None,
                dev_anon: false,
            },
            &[],
        )
        .await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/conversations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch protected request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn protected_route_allows_dev_anon_opt_in() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: None,
                dev_anon: true,
            },
            &[],
        )
        .await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/conversations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch protected request");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn protected_route_rejects_missing_or_wrong_bearer_token() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: Some("secret-token".to_string()),
                dev_anon: false,
            },
            &[],
        )
        .await;

        let missing = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/conversations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch missing token request");
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let wrong = app
            .oneshot(
                Request::builder()
                    .uri("/v1/conversations")
                    .header(header::AUTHORIZATION, "Bearer wrong-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch wrong token request");
        assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_bearer_token_succeeds_for_read_and_write_routes() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: Some("secret-token".to_string()),
                dev_anon: false,
            },
            &[],
        )
        .await;

        let read = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/conversations")
                    .header(header::AUTHORIZATION, "Bearer secret-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("dispatch read request");
        assert_eq!(read.status(), StatusCode::OK);

        let write = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/events")
                    .header(header::AUTHORIZATION, "Bearer secret-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "event_name": "test_event"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .expect("dispatch write request");
        assert_eq!(write.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn untrusted_origin_does_not_receive_cors_headers_by_default() {
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: None,
                dev_anon: true,
            },
            &[],
        )
        .await;

        let response = app
            .oneshot(preflight_request("http://evil.example"))
            .await
            .expect("dispatch preflight request");

        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }

    #[tokio::test]
    async fn trusted_origin_receives_cors_headers_when_configured() {
        let trusted_origin = "https://trusted.example";
        let (_tmp, app) = test_app(
            AuthConfig {
                api_token: None,
                dev_anon: true,
            },
            &[trusted_origin],
        )
        .await;

        let response = app
            .oneshot(preflight_request(trusted_origin))
            .await
            .expect("dispatch trusted preflight request");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("https://trusted.example"))
        );
        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .is_some());
    }

    async fn test_app(auth: AuthConfig, trusted_origins: &[&str]) -> (TempDir, Router) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let db_path = temp_dir.path().join("refine-server.sqlite");
        let state = Arc::new(
            AppState::build_for_test(db_path, auth)
                .await
                .expect("build app state"),
        );
        let app = build_app(state, allow_origins(trusted_origins));
        (temp_dir, app)
    }

    fn allow_origins(origins: &[&str]) -> AllowOrigin {
        AllowOrigin::list(
            origins
                .iter()
                .map(|origin| origin.parse::<HeaderValue>().expect("valid origin"))
                .collect::<Vec<_>>(),
        )
    }

    fn preflight_request(origin: &str) -> Request<Body> {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/v1/conversations")
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap()
    }
}
