//! 搜索查询对象
//!
//! 封装搜索参数

use crate::knowledge::ItemType;

/// 搜索查询
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// 搜索关键词
    pub text: String,
    /// 过滤条件
    pub filter: SearchFilter,
    /// 分页
    pub pagination: Pagination,
}

impl SearchQuery {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            filter: SearchFilter::default(),
            pagination: Pagination::default(),
        }
    }

    pub fn with_type(mut self, item_type: ItemType) -> Self {
        self.filter.item_type = Some(item_type);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.filter.tags = tags;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.pagination.limit = limit;
        self
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.pagination.offset = offset;
        self
    }
}

/// 搜索过滤条件
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    /// 按类型过滤
    pub item_type: Option<ItemType>,
    /// 按标签过滤
    pub tags: Vec<String>,
}

/// 分页参数
#[derive(Debug, Clone)]
pub struct Pagination {
    pub limit: usize,
    pub offset: usize,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            limit: 20,
            offset: 0,
        }
    }
}

/// 搜索结果
#[derive(Debug, Clone)]
pub struct SearchResult<T> {
    /// 结果列表
    pub items: Vec<SearchHit<T>>,
    /// 总数
    pub total: usize,
    /// 查询参数
    pub query: SearchQuery,
}

impl<T> SearchResult<T> {
    pub fn empty(query: SearchQuery) -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            query,
        }
    }
}

/// 单个搜索命中
#[derive(Debug, Clone)]
pub struct SearchHit<T> {
    /// 结果项
    pub item: T,
    /// 相关度分数 (0-1)
    pub score: f32,
}

impl<T> SearchHit<T> {
    pub fn new(item: T, score: f32) -> Self {
        Self { item, score }
    }
}
