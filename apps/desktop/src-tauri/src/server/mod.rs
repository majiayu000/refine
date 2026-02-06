//! 本地 HTTP API 服务器
//!
//! 提供 HTTP API 供浏览器扩展调用

mod extract;
mod http;

use extract::build_llm_client_from_env;
use refine_core::infra::{LlmClient, SqliteStore};
use std::sync::Arc;
use tiny_http::Server;
use tokio::runtime::Builder as RuntimeBuilder;

const SERVER_PORT: u16 = 19527;
const MAX_SERVER_WORKERS: usize = 8;

/// 启动 HTTP 服务器
pub fn start_server(store: Arc<SqliteStore>) {
    std::thread::spawn(move || {
        let server = match Server::http(format!("127.0.0.1:{}", SERVER_PORT)) {
            Ok(server) => Arc::new(server),
            Err(err) => {
                eprintln!("Failed to start HTTP server: {}", err);
                return;
            }
        };

        let llm_client = match build_llm_client_from_env() {
            Ok(client) => Some(client),
            Err(err) => {
                eprintln!("LLM is not configured, /extract will return errors: {}", err);
                None
            }
        };

        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get().clamp(1, MAX_SERVER_WORKERS))
            .unwrap_or(4);

        println!(
            "Refine API: http://localhost:{} (workers: {})",
            SERVER_PORT, worker_count
        );

        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let server = Arc::clone(&server);
            let store = Arc::clone(&store);
            let llm_client = llm_client.clone();
            handles.push(std::thread::spawn(move || {
                run_worker(server, store, llm_client);
            }));
        }

        for handle in handles {
            let _ = handle.join();
        }
    });
}

fn run_worker(server: Arc<Server>, store: Arc<SqliteStore>, llm_client: Option<Arc<dyn LlmClient>>) {
    let runtime = match RuntimeBuilder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("Failed to create HTTP runtime: {}", err);
            return;
        }
    };

    loop {
        let mut request = match server.recv() {
            Ok(request) => request,
            Err(_) => break,
        };

        let response = http::handle_request(&mut request, &store, &runtime, llm_client.as_ref());
        let _ = request.respond(response);
    }
}
