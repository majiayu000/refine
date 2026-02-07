use refine_core::knowledge::Item;
use serde::Serialize;

/// 前端使用的 Item DTO
#[derive(Serialize)]
pub struct ItemDto {
    pub id: String,
    pub item_type: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: String,
}

impl From<&Item> for ItemDto {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id().as_str().to_string(),
            item_type: format!("{:?}", item.item_type()).to_lowercase(),
            title: item.title().to_string(),
            summary: item.summary().to_string(),
            content: item.content().to_string(),
            tags: item
                .tags()
                .iter()
                .map(|tag| tag.as_str().to_string())
                .collect(),
            created_at: item.created_at().to_rfc3339(),
        }
    }
}

/// 搜索结果 DTO
#[derive(Serialize)]
pub struct SearchResultDto {
    pub items: Vec<ItemDto>,
    pub total: usize,
}
