//! Embed the standalone API so extension behavior is identical in both installations.

use crate::app::AppState;

pub fn start_server(state: AppState) {
    let port = std::env::var("REFINE_DESKTOP_API_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(21567);
    tauri::async_runtime::spawn(async move {
        if let Err(error) = refine_server::serve_embedded(state, port).await {
            tracing::error!(%error, "Desktop HTTP API stopped");
        }
    });
}
