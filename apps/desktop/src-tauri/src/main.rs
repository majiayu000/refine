//! Refine 桌面应用
//!
//! Tauri 2.0 桌面应用

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod server;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("refine=debug,refine_core=debug")
        .init();

    let state = app::build_state();

    // 启动 HTTP API 服务器（供浏览器扩展调用）
    server::start_server(state.store.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app::commands::get_items,
            app::commands::get_item,
            app::commands::search_items,
            app::commands::create_item,
            app::commands::update_item,
            app::commands::delete_item,
        ])
        .run(tauri::generate_context!())
        .expect("启动应用失败");
}
