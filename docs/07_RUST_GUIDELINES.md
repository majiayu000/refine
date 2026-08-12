# Refine - Rust 编码规范

> 核心库 (packages/core) 的 Rust 最佳实践

---

## 1. 项目结构

```
packages/core/
├── Cargo.toml
├── src/
│   ├── lib.rs              # 库入口，导出公共 API
│   ├── error.rs            # 统一错误类型
│   ├── config.rs           # 配置管理
│   │
│   ├── models/             # 数据模型
│   │   ├── mod.rs
│   │   ├── item.rs         # 知识片段
│   │   ├── skill.rs        # 技能
│   │   └── snippet.rs      # 代码片段
│   │
│   ├── storage/            # 存储层
│   │   ├── mod.rs
│   │   ├── database.rs     # SQLite 操作
│   │   └── vector.rs       # 向量索引
│   │
│   ├── extractor/          # 知识提取
│   │   ├── mod.rs
│   │   ├── parser.rs       # 对话解析
│   │   └── refiner.rs      # LLM 提炼
│   │
│   ├── search/             # 搜索引擎
│   │   ├── mod.rs
│   │   ├── keyword.rs      # 关键词搜索
│   │   └── semantic.rs     # 语义搜索
│   │
│   └── llm/                # LLM 集成
│       ├── mod.rs
│       ├── client.rs       # API 客户端
│       ├── openai.rs       # OpenAI 实现
│       └── claude.rs       # Claude 实现
│
└── tests/
    ├── integration/
    └── fixtures/
```

---

## 2. 错误处理

### 2.1 使用 thiserror 定义错误

```rust
// src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RefineError {
    // 存储错误
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Item not found: {0}")]
    NotFound(String),

    // LLM 错误
    #[error("LLM request failed: {0}")]
    LlmRequest(String),

    #[error("LLM response parse error: {message}")]
    LlmParse { message: String },

    // 提取错误
    #[error("Failed to extract knowledge: {0}")]
    Extraction(String),

    // 配置错误
    #[error("Configuration error: {0}")]
    Config(String),

    // 序列化错误
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    // IO 错误
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RefineError>;
```

### 2.2 使用 ? 和 context

```rust
use crate::error::{RefineError, Result};

impl Storage {
    pub async fn get_item(&self, id: &str) -> Result<Item> {
        self.db
            .get(id)
            .await?
            .ok_or_else(|| RefineError::NotFound(id.to_string()))
    }
}
```

### 2.3 绝不在库代码中 panic

```rust
// ❌ BAD
pub fn get_item(&self, index: usize) -> &Item {
    &self.items[index]  // 可能 panic
}

// ✅ GOOD
pub fn get_item(&self, index: usize) -> Option<&Item> {
    self.items.get(index)
}

// ✅ GOOD - 需要返回 Result 时
pub fn get_item(&self, index: usize) -> Result<&Item> {
    self.items
        .get(index)
        .ok_or(RefineError::NotFound(format!("index: {}", index)))
}
```

---

## 3. 数据模型

### 3.1 使用 Newtype 模式

```rust
// src/models/item.rs

/// 知识片段 ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(String);

impl ItemId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

### 3.2 使用枚举表示类型

```rust
/// 知识片段类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    Knowledge,
    Skill,
    Snippet,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Skill => "skill",
            Self::Snippet => "snippet",
        }
    }
}
```

### 3.3 使用 Builder 模式构造复杂对象

```rust
// src/models/item.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub item_type: ItemType,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub source: Option<Source>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct ItemBuilder {
    item_type: ItemType,
    title: String,
    content: String,
    tags: Vec<String>,
    source: Option<Source>,
}

impl ItemBuilder {
    pub fn new(item_type: ItemType, title: impl Into<String>) -> Self {
        Self {
            item_type,
            title: title.into(),
            content: String::new(),
            tags: Vec::new(),
            source: None,
        }
    }

    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    pub fn source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }

    pub fn build(self) -> Item {
        let now = Utc::now();
        Item {
            id: ItemId::new(),
            item_type: self.item_type,
            title: self.title,
            content: self.content,
            tags: self.tags,
            source: self.source,
            created_at: now,
            updated_at: now,
        }
    }
}

// 使用
let item = ItemBuilder::new(ItemType::Knowledge, "Python asyncio 指南")
    .content("...")
    .tags(["python", "asyncio"])
    .build();
```

---

## 4. API 设计

### 4.1 接受 impl Trait 增加灵活性

```rust
// ❌ BAD - 太具体
pub fn search(&self, query: String) -> Result<Vec<Item>> { ... }

// ✅ GOOD - 接受任何可转为 &str 的类型
pub fn search(&self, query: impl AsRef<str>) -> Result<Vec<Item>> {
    let query = query.as_ref();
    // ...
}

// ❌ BAD
pub fn add_tags(&mut self, tags: Vec<String>) { ... }

// ✅ GOOD - 接受任何可迭代的类型
pub fn add_tags(&mut self, tags: impl IntoIterator<Item = impl Into<String>>) {
    self.tags.extend(tags.into_iter().map(Into::into));
}
```

### 4.2 使用 Cow 避免不必要的分配

```rust
use std::borrow::Cow;

/// 标准化搜索查询
pub fn normalize_query(input: &str) -> Cow<'_, str> {
    if input.chars().all(|c| c.is_lowercase() && !c.is_whitespace()) {
        // 已经是标准格式，返回借用
        Cow::Borrowed(input)
    } else {
        // 需要转换，返回所有权
        Cow::Owned(input.to_lowercase().trim().to_string())
    }
}
```

### 4.3 异步 trait 使用 async-trait

```rust
use async_trait::async_trait;

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;
}

pub struct OpenAiClient { /* ... */ }

#[async_trait]
impl LlmClient for OpenAiClient {
    async fn complete(&self, prompt: &str) -> Result<String> {
        // ...
    }

    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // ...
    }
}
```

---

## 5. 性能优化

### 5.1 避免不必要的 clone

```rust
// ❌ BAD
fn process(data: &String) {
    let owned = data.clone();  // 不必要的分配
    do_something(&owned);
}

// ✅ GOOD
fn process(data: &str) {
    do_something(data);
}
```

### 5.2 使用迭代器而非循环

```rust
// ❌ BAD
let mut results = Vec::new();
for item in items {
    if item.is_valid() {
        results.push(item.transform());
    }
}

// ✅ GOOD
let results: Vec<_> = items
    .into_iter()
    .filter(|item| item.is_valid())
    .map(|item| item.transform())
    .collect();
```

### 5.3 预分配容量

```rust
// ❌ BAD - 多次重新分配
let mut results = Vec::new();
for i in 0..1000 {
    results.push(compute(i));
}

// ✅ GOOD - 一次分配
let mut results = Vec::with_capacity(1000);
for i in 0..1000 {
    results.push(compute(i));
}

// ✅ BETTER - 使用迭代器
let results: Vec<_> = (0..1000).map(compute).collect();
```

### 5.4 使用 Arc 共享只读数据

```rust
use std::sync::Arc;

pub struct SearchEngine {
    // 配置只读，多线程共享
    config: Arc<Config>,
    // 索引需要可变，用 RwLock
    index: Arc<RwLock<Index>>,
}

impl SearchEngine {
    pub async fn search(&self, query: &str) -> Result<Vec<Item>> {
        let index = self.index.read().await;
        // ...
    }
}
```

---

## 6. 异步编程

### 6.1 使用 tokio 运行时

```rust
// Cargo.toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

### 6.2 并发执行独立任务

```rust
use tokio::task::JoinSet;

pub async fn embed_all(texts: Vec<String>, client: &impl LlmClient) -> Result<Vec<Vec<f32>>> {
    let mut set = JoinSet::new();

    for text in texts {
        let client = client.clone();
        set.spawn(async move {
            client.embed(&text).await
        });
    }

    let mut results = Vec::with_capacity(set.len());
    while let Some(res) = set.join_next().await {
        results.push(res??);
    }

    Ok(results)
}
```

### 6.3 使用 tracing 记录日志

```rust
use tracing::{info, instrument, warn};

#[instrument(skip(self, content), fields(content_len = content.len()))]
pub async fn extract(&self, content: &str) -> Result<ExtractedKnowledge> {
    info!("Starting extraction");

    let result = self.llm.complete(&self.build_prompt(content)).await?;

    if result.is_empty() {
        warn!("Empty extraction result");
    }

    Ok(self.parse_result(&result)?)
}
```

---

## 7. 测试

### 7.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_id_generation() {
        let id1 = ItemId::new();
        let id2 = ItemId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_item_builder() {
        let item = ItemBuilder::new(ItemType::Knowledge, "Test")
            .content("Content")
            .tags(["tag1", "tag2"])
            .build();

        assert_eq!(item.title, "Test");
        assert_eq!(item.tags.len(), 2);
    }
}
```

### 7.2 异步测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search() {
        let engine = SearchEngine::new_for_test().await;
        engine.add_item(test_item()).await.unwrap();

        let results = engine.search("test").await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
```

### 7.3 使用 mockall 模拟依赖

```rust
use mockall::automock;

#[automock]
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extractor_with_mock() {
        let mut mock = MockLlmClient::new();
        mock.expect_complete()
            .returning(|_| Ok(r#"{"title": "Test", "summary": "..."}"#.to_string()));

        let extractor = Extractor::new(mock);
        let result = extractor.extract("input").await.unwrap();

        assert_eq!(result.title, "Test");
    }
}
```

---

## 8. 依赖管理

### 8.1 Cargo.toml 推荐配置

```toml
[package]
name = "refine-core"
version = "0.1.0"
edition = "2021"
rust-version = "1.88"

[dependencies]
# 异步运行时
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"

# 错误处理
thiserror = "1"
anyhow = "1"

# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# 数据库
rusqlite = { version = "0.31", features = ["bundled"] }

# 日志
tracing = "0.1"
tracing-subscriber = "0.3"

# HTTP 客户端
reqwest = { version = "0.12", features = ["json"] }

# 工具
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
mockall = "0.12"
tempfile = "3"
```

### 8.2 Feature Flags

```toml
[features]
default = ["sqlite"]
sqlite = ["rusqlite"]
postgres = ["sqlx"]  # 未来扩展
```

---

## 9. 代码风格

### 9.1 使用 rustfmt

```toml
# rustfmt.toml
edition = "2021"
max_width = 100
tab_spaces = 4
use_small_heuristics = "Default"
```

### 9.2 使用 clippy

```bash
cargo clippy -- -W clippy::all -W clippy::pedantic
```

### 9.3 常用 clippy lint

```rust
#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]  // 允许 item::Item
```

---

## 10. 常见反模式

| 反模式 | 正确做法 |
|-------|---------|
| 到处 `unwrap()` | 使用 `?` 和正确的错误类型 |
| `clone()` 满足借用检查 | 重构代码结构，使用引用 |
| `Box<dyn Error>` | 用 `thiserror` 定义具体错误 |
| 所有文本用 `String` | 用 `&str`、`Cow<str>` 或领域类型 |
| 手动 `Drop` 清理 | RAII 模式，利用析构函数 |
| 无理由的 `unsafe` | 先尝试安全抽象 |
| 过度使用 `Arc<Mutex<_>>` | 消息传递、channel |
| 在 async 中阻塞 | 用 `spawn_blocking` 处理 CPU 密集任务 |
