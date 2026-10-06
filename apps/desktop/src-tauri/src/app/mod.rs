pub mod commands;
mod dto;
mod state;

pub use state::AppState;

pub fn build_state() -> AppState {
    let db_path = refine_core::infra::resolve_db_path(&["REFINE_DESKTOP_DB_PATH"]);
    std::sync::Arc::new(
        tauri::async_runtime::block_on(refine_server::AppState::build_at(db_path))
            .expect("无法初始化 Refine 服务"),
    )
}
