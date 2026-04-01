mod api_response;
mod application;
mod auth;
mod extraction;
mod handlers;
mod models;
mod persistence;
mod request_guard;
mod state;
mod vector_search;

use axum::http::{header, Method};
use axum::routing::{delete, get, post};
use axum::Router;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

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

    let cors = CorsLayer::new()
        .allow_origin(Any)
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
        .max_age(std::time::Duration::from_secs(60 * 60));

    let app = Router::new()
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
        .layer(cors)
        .with_state(state.clone());

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
        eprintln!("Port {} is occupied by another process, trying next...", port);
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
            .map_or(false, |body| body.contains("Refine cloud API")),
        Err(_) => false,
    }
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
    use super::{parse_bind_addr, requires_api_token_for_bind};
    use std::net::SocketAddr;

    #[test]
    fn parse_bind_addr_falls_back_to_loopback_for_invalid_host() {
        let addr = parse_bind_addr("localhost", 5567);
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 5567)));
    }

    #[test]
    fn non_loopback_bindings_require_api_token() {
        assert!(requires_api_token_for_bind(&SocketAddr::from((
            [0, 0, 0, 0],
            5567
        ))));
        assert!(requires_api_token_for_bind(&SocketAddr::from((
            [192, 168, 1, 8],
            5567
        ))));
        assert!(!requires_api_token_for_bind(&SocketAddr::from((
            [127, 0, 0, 1],
            5567
        ))));
        assert!(!requires_api_token_for_bind(&SocketAddr::from((
            [0, 0, 0, 0, 0, 0, 0, 1],
            5567
        ))));
    }
}
