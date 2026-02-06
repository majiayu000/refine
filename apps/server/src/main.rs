mod auth;
mod extraction;
mod handlers;
mod models;
mod persistence;
mod state;

use axum::http::{header, Method};
use axum::routing::{get, post};
use axum::Router;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter("refine_server=info,refine_core=info")
        .init();

    let state = match AppState::build() {
        Ok(state) => Arc::new(state),
        Err(err) => {
            eprintln!("Failed to initialize app state: {}", err);
            std::process::exit(1);
        }
    };

    if state.llm_client.is_none() {
        println!(
            "LLM is not configured, server will use fallback extraction. Set REFINE_ANTHROPIC_API_KEY or REFINE_OPENAI_API_KEY to enable LLM extraction."
        );
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::HeaderName::from_static("x-refine-client"),
        ])
        .expose_headers([header::CONTENT_TYPE])
        .max_age(std::time::Duration::from_secs(60 * 60));

    let app = Router::new()
        .route("/", get(handlers::dashboard_page))
        .route("/dashboard", get(handlers::dashboard_page))
        .route("/health", get(handlers::health))
        .route(
            "/v1/conversations",
            get(handlers::list_conversations).post(handlers::create_conversation),
        )
        .route("/v1/extraction-jobs", post(handlers::create_extraction_job))
        .route(
            "/v1/extraction-jobs/:job_id",
            get(handlers::get_extraction_job),
        )
        .route("/v1/items", get(handlers::list_items))
        .route("/v1/search", get(handlers::search_items))
        .layer(cors)
        .with_state(state);

    let port = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8787);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], port)));

    println!("Refine cloud API (Rust) listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind tcp listener");
    axum::serve(listener, app).await.expect("server exited");
}
