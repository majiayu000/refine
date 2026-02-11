use serde::Serialize;

/// 前端使用的 Item DTO（复用共享契约）
pub type ItemDto = refine_core::infra::ItemDto;

/// 搜索结果 DTO
#[derive(Serialize)]
pub struct SearchResultDto {
    pub items: Vec<ItemDto>,
    pub total: usize,
}
