use serde::Serialize;

/// 前端使用的 Item DTO（复用共享契约）
pub type ItemDto = refine_core::infra::ItemDto;

/// 列表结果 DTO（含分页信息）
#[derive(Serialize)]
pub struct ItemListResultDto {
    pub items: Vec<ItemDto>,
    pub total: usize,
    pub next_cursor: Option<usize>,
}

/// 搜索结果 DTO
#[derive(Serialize)]
pub struct SearchResultDto {
    pub items: Vec<ItemDto>,
    pub total: usize,
}
