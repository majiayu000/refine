use super::dto::{ItemDto, ItemListResultDto, SearchResultDto};
use super::state::AppState;
use refine_core::knowledge::{Item, ItemId, ItemType, Tag};
use refine_core::search::SearchQuery;
use tauri::State;

#[tauri::command]
pub async fn get_items(
    state: State<'_, AppState>,
    item_type: Option<String>,
    cursor: Option<usize>,
    limit: Option<usize>,
) -> Result<ItemListResultDto, String> {
    let cursor = cursor.unwrap_or(0);
    let limit = limit.unwrap_or(50).clamp(1, 100);

    let item_type_filter = match item_type {
        Some(type_str) => Some(match type_str.as_str() {
            "knowledge" => ItemType::Knowledge,
            "skill" => ItemType::Skill,
            "snippet" => ItemType::Snippet,
            _ => return Err("无效的类型".to_string()),
        }),
        None => None,
    };

    let total = state
        .store
        .count_items(item_type_filter)
        .await
        .map_err(|e| e.to_string())?;
    let items = state
        .store
        .find_recent(item_type_filter, cursor, limit)
        .await
        .map_err(|e| e.to_string())?;
    let next_cursor = if cursor + items.len() < total {
        Some(cursor + items.len())
    } else {
        None
    };

    Ok(ItemListResultDto {
        items: items.iter().map(ItemDto::from).collect(),
        total,
        next_cursor,
    })
}

#[tauri::command]
pub async fn get_item(state: State<'_, AppState>, id: String) -> Result<Option<ItemDto>, String> {
    let item_id = ItemId::from(id.as_str());

    state
        .store
        .find_by_id(&item_id)
        .await
        .map(|opt| opt.map(|item| ItemDto::from(&item)))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_items(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<SearchResultDto, String> {
    let limit = limit.unwrap_or(20);

    let results = state
        .engine
        .search(SearchQuery::new(&query).with_limit(limit))
        .await
        .map_err(|e| e.to_string())?;

    Ok(SearchResultDto {
        items: results
            .items
            .iter()
            .map(|hit| ItemDto::from(&hit.item))
            .collect(),
        total: results.total,
    })
}

#[tauri::command]
pub async fn create_item(
    state: State<'_, AppState>,
    title: String,
    summary: String,
    content: String,
    item_type: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<ItemDto, String> {
    let item_type = item_type
        .map(|item_type| match item_type.as_str() {
            "skill" => ItemType::Skill,
            "snippet" => ItemType::Snippet,
            _ => ItemType::Knowledge,
        })
        .unwrap_or(ItemType::Knowledge);

    let mut item = match item_type {
        ItemType::Knowledge => Item::new_knowledge(&title, &summary),
        ItemType::Skill => Item::new_skill(&title, &summary),
        ItemType::Snippet => Item::new_snippet(&title, &summary),
        ItemType::Observation => Item::new_observation(&title, &summary),
    };

    item.set_content(&content);

    if let Some(tag_strs) = tags {
        let tags: Vec<Tag> = tag_strs
            .iter()
            .filter_map(|tag| Tag::try_new(tag))
            .collect();
        let _ = item.set_tags(tags);
    }

    state.store.save(&item).await.map_err(|e| e.to_string())?;
    Ok(ItemDto::from(&item))
}

#[tauri::command]
pub async fn update_item(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    summary: Option<String>,
    content: Option<String>,
) -> Result<ItemDto, String> {
    let item_id = ItemId::from(id.as_str());

    let mut item = state
        .store
        .find_by_id(&item_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未找到该知识".to_string())?;

    if let Some(title) = title {
        item.set_title(&title);
    }
    if let Some(summary) = summary {
        item.set_summary(&summary);
    }
    if let Some(content) = content {
        item.set_content(&content);
    }

    state.store.save(&item).await.map_err(|e| e.to_string())?;
    Ok(ItemDto::from(&item))
}

#[tauri::command]
pub async fn delete_item(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    let item_id = ItemId::from(id.as_str());
    state
        .store
        .delete(&item_id)
        .await
        .map_err(|e| e.to_string())
}
