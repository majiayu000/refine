//! Refine 桌面应用
//!
//! Tauri 2.0 桌面应用

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemId, ItemRepository, ItemType};
use refine_core::search::{SearchEngine, SearchQuery};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// 应用状态
struct AppState {
    store: Arc<SqliteStore>,
    engine: SearchEngine,
}

/// 前端使用的 Item DTO
#[derive(Serialize)]
struct ItemDto {
    id: String,
    item_type: String,
    title: String,
    summary: String,
    content: String,
    tags: Vec<String>,
    created_at: String,
}

impl From<&Item> for ItemDto {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id().as_str().to_string(),
            item_type: format!("{:?}", item.item_type()).to_lowercase(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            tags: item.tags().iter().map(|t| t.as_str().to_string()).collect(),
            created_at: item.created_at().to_rfc3339(),
        }
    }
}

/// 搜索结果 DTO
#[derive(Serialize)]
struct SearchResultDto {
    items: Vec<ItemDto>,
    total: usize,
}

fn get_db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("refine")
        .join("data.db")
}

fn ensure_db_dir(path: &PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

// ============ Tauri 命令 ============

#[tauri::command]
async fn get_items(
    state: State<'_, Mutex<AppState>>,
    item_type: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<ItemDto>, String> {
    let state = state.lock().await;
    let limit = limit.unwrap_or(50);

    let items = if let Some(type_str) = item_type {
        let item_type = match type_str.as_str() {
            "knowledge" => ItemType::Knowledge,
            "skill" => ItemType::Skill,
            "snippet" => ItemType::Snippet,
            _ => return Err("无效的类型".to_string()),
        };
        state.store.find_by_type(item_type).await
    } else {
        state.store.find_all().await
    };

    items
        .map(|items| items.iter().take(limit).map(ItemDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_item(state: State<'_, Mutex<AppState>>, id: String) -> Result<Option<ItemDto>, String> {
    let state = state.lock().await;
    let item_id = ItemId::from_str(&id);

    state
        .store
        .find_by_id(&item_id)
        .await
        .map(|opt| opt.map(|item| ItemDto::from(&item)))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_items(
    state: State<'_, Mutex<AppState>>,
    query: String,
    limit: Option<usize>,
) -> Result<SearchResultDto, String> {
    let state = state.lock().await;
    let limit = limit.unwrap_or(20);

    let results = state
        .engine
        .search(SearchQuery::new(&query).with_limit(limit))
        .await
        .map_err(|e| e.to_string())?;

    Ok(SearchResultDto {
        items: results.items.iter().map(|h| ItemDto::from(&h.item)).collect(),
        total: results.total,
    })
}

#[tauri::command]
async fn create_item(
    state: State<'_, Mutex<AppState>>,
    title: String,
    summary: String,
    content: String,
    item_type: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<ItemDto, String> {
    let state = state.lock().await;

    let item_type = item_type
        .map(|t| match t.as_str() {
            "skill" => ItemType::Skill,
            "snippet" => ItemType::Snippet,
            _ => ItemType::Knowledge,
        })
        .unwrap_or(ItemType::Knowledge);

    let mut item = match item_type {
        ItemType::Knowledge => Item::new_knowledge(&title, &summary),
        ItemType::Skill => Item::new_skill(&title, &summary),
        ItemType::Snippet => Item::new_snippet(&title, &summary),
    };

    item.set_content(&content);

    if let Some(tag_strs) = tags {
        let tags: Vec<_> = tag_strs
            .iter()
            .filter_map(|t| refine_core::knowledge::Tag::try_new(t))
            .collect();
        let _ = item.set_tags(tags);
    }

    state.store.save(&item).await.map_err(|e| e.to_string())?;

    Ok(ItemDto::from(&item))
}

#[tauri::command]
async fn update_item(
    state: State<'_, Mutex<AppState>>,
    id: String,
    title: Option<String>,
    summary: Option<String>,
    content: Option<String>,
) -> Result<ItemDto, String> {
    let state = state.lock().await;
    let item_id = ItemId::from_str(&id);

    let mut item = state
        .store
        .find_by_id(&item_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("未找到该知识")?;

    if let Some(t) = title {
        item.set_title(&t);
    }
    if let Some(s) = summary {
        item.set_summary(&s);
    }
    if let Some(c) = content {
        item.set_content(&c);
    }

    state.store.save(&item).await.map_err(|e| e.to_string())?;

    Ok(ItemDto::from(&item))
}

#[tauri::command]
async fn delete_item(state: State<'_, Mutex<AppState>>, id: String) -> Result<bool, String> {
    let state = state.lock().await;
    let item_id = ItemId::from_str(&id);

    state
        .store
        .delete(&item_id)
        .await
        .map_err(|e| e.to_string())
}

// ============ 主函数 ============

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("refine=debug,refine_core=debug")
        .init();

    let db_path = get_db_path();
    ensure_db_dir(&db_path);

    let store = SqliteStore::open(&db_path).expect("打开数据库失败");
    let store = Arc::new(store);
    let engine = SearchEngine::new(store.clone());

    // 启动 HTTP API 服务器（供浏览器扩展调用）
    server::start_server(store.clone());

    let state = Mutex::new(AppState { store, engine });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_items,
            get_item,
            search_items,
            create_item,
            update_item,
            delete_item,
        ])
        .run(tauri::generate_context!())
        .expect("启动应用失败");
}
