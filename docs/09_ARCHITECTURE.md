# Refine - 模块化架构设计

> 严格模块化，固定文件结构，每个文件不超过 200 行

---

## 1. 设计原则

```
┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│  1. 按业务领域划分模块                                               │
│     ✗ models/, services/, repositories/                             │
│     ✓ knowledge/, refinement/, search/, infra/                      │
│                                                                     │
│  2. 固定文件结构，禁止随意新增                                        │
│     • 每个模块的文件在 mod.rs 中明确列出                              │
│     • 新增文件需要充分理由                                           │
│                                                                     │
│  3. 每个文件不超过 200 行                                            │
│     • 强制解耦和单一职责                                             │
│     • 超过则拆分                                                    │
│                                                                     │
│  4. 接口在领域层，实现在基础设施层                                    │
│     • knowledge/repository.rs 定义 trait                            │
│     • infra/sqlite.rs 实现 trait                                    │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 2. 项目结构

```
refine/
├── docs/                           # 文档
│   ├── 00_OVERVIEW.md
│   ├── 01_PRD.md
│   ├── ...
│   └── 09_ARCHITECTURE.md
│
├── packages/
│   └── core/                       # Rust 核心库
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs              # 主入口（禁止新增顶层模块）
│           ├── error.rs            # 统一错误类型
│           ├── knowledge/          # 知识管理模块
│           ├── refinement/         # 知识提炼模块
│           ├── search/             # 搜索模块
│           └── infra/              # 基础设施模块
│
├── apps/
│   ├── cli/                        # CLI 应用
│   ├── desktop/                    # Tauri 桌面应用
│   └── extension/                  # 浏览器插件 (Plasmo)
│
└── Cargo.toml                      # Workspace 配置
```

---

## 3. 核心库模块详情

### 3.1 模块地图

```
┌─────────────────────────────────────────────────────────────────────┐
│                       refine-core 模块结构                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│   lib.rs ─────────────────────────────────────────────────────────  │
│     │                                                               │
│     ├── error.rs          统一错误类型 (62 行)                       │
│     │                                                               │
│     ├── knowledge/        知识管理                                   │
│     │   ├── mod.rs        模块入口 (17 行)                          │
│     │   ├── types.rs      值对象: ItemId, Tag, Source (117 行)      │
│     │   ├── item.rs       Item 聚合根 (156 行)                      │
│     │   └── repository.rs 仓储接口 (38 行)                          │
│     │                                                               │
│     ├── refinement/       知识提炼                                   │
│     │   ├── mod.rs        模块入口 (17 行)                          │
│     │   ├── conversation.rs 对话解析 (198 行)                       │
│     │   ├── policy.rs     提炼策略 (104 行)                         │
│     │   └── extractor.rs  提炼器 (146 行)                           │
│     │                                                               │
│     ├── search/           搜索引擎                                   │
│     │   ├── mod.rs        模块入口 (14 行)                          │
│     │   ├── query.rs      查询对象 (108 行)                         │
│     │   └── engine.rs     搜索引擎 (156 行)                         │
│     │                                                               │
│     └── infra/            基础设施                                   │
│         ├── mod.rs        模块入口 (14 行)                          │
│         ├── sqlite.rs     SQLite 存储 (210 行)                      │
│         ├── llm.rs        LLM 客户端 (209 行)                       │
│         └── schema.sql    数据库 Schema (45 行)                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### 3.2 模块职责

| 模块 | 职责 | 关键类型 |
|------|------|----------|
| `error` | 统一错误处理 | `DomainError`, `InfraError`, `AppError` |
| `knowledge` | 知识片段管理 | `Item`, `ItemId`, `Tag`, `Source`, `ItemRepository` |
| `refinement` | 从对话提炼知识 | `Conversation`, `Extractor`, `ExtractionPolicy` |
| `search` | 关键词+语义搜索 | `SearchQuery`, `SearchEngine`, `SearchResult` |
| `infra` | 技术实现 | `SqliteStore`, `ClaudeClient`, `OpenAIClient` |

---

## 4. 文件结构约束

### 4.1 knowledge 模块（固定 4 个文件）

```rust
// knowledge/mod.rs
//! 知识管理模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `types.rs` - 值对象 (ItemId, Tag, Source, ItemType)
//! - `item.rs` - Item 聚合根
//! - `repository.rs` - 仓储接口

mod item;
mod repository;
mod types;

pub use item::Item;
pub use repository::ItemRepository;
pub use types::{ItemId, ItemType, Source, Tag};
```

### 4.2 refinement 模块（固定 4 个文件）

```rust
// refinement/mod.rs
//! 知识提炼模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `conversation.rs` - 对话解析
//! - `policy.rs` - 提炼策略
//! - `extractor.rs` - 提炼器

mod conversation;
mod extractor;
mod policy;

pub use conversation::{Conversation, Message, Role};
pub use extractor::{ExtractionResult, Extractor};
pub use policy::{ExtractionPolicy, PromptTemplate};
```

### 4.3 search 模块（固定 3 个文件）

```rust
// search/mod.rs
//! 搜索模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `query.rs` - 查询对象
//! - `engine.rs` - 搜索引擎

mod engine;
mod query;

pub use engine::{SearchEngine, VectorSearch};
pub use query::{Pagination, SearchFilter, SearchHit, SearchQuery, SearchResult};
```

### 4.4 infra 模块（固定 4 个文件）

```rust
// infra/mod.rs
//! 基础设施模块
//!
//! ## 文件结构（固定，禁止新增）
//!
//! - `sqlite.rs` - SQLite 存储实现
//! - `llm.rs` - LLM 客户端 (Claude, OpenAI)
//! - `schema.sql` - 数据库 Schema

mod llm;
mod sqlite;

pub use llm::{ClaudeClient, LlmClient, OpenAIClient};
pub use sqlite::SqliteStore;
```

---

## 5. 依赖规则

```
┌─────────────────────────────────────────────────────────────────────┐
│                          依赖方向图                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│          ┌───────────┐ ┌───────────┐ ┌───────────┐                 │
│          │ knowledge │ │refinement │ │  search   │                 │
│          └─────┬─────┘ └─────┬─────┘ └─────┬─────┘                 │
│                │             │             │                        │
│                └─────────────┼─────────────┘                        │
│                              │                                      │
│                              ▼                                      │
│                         ┌─────────┐                                 │
│                         │  error  │                                 │
│                         └────┬────┘                                 │
│                              │                                      │
│                              ▼                                      │
│                         ┌─────────┐                                 │
│                         │  infra  │  实现 knowledge 的接口           │
│                         └─────────┘                                 │
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ✓ 所有模块依赖 error                                               │
│  ✓ refinement 依赖 knowledge（使用 Item 类型）                       │
│  ✓ search 依赖 knowledge（使用 Item, ItemRepository）                │
│  ✓ infra 实现 knowledge::ItemRepository                             │
│  ✗ knowledge 不依赖其他业务模块                                      │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 6. 核心类型

### 6.1 Item（知识片段）

```rust
pub struct Item {
    id: ItemId,
    item_type: ItemType,
    title: String,
    summary: String,
    content: String,
    tags: Vec<Tag>,
    source: Option<Source>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub enum ItemType {
    Knowledge,  // 知识概念
    Skill,      // 可执行技能（prompt 模板）
    Snippet,    // 代码片段
}
```

### 6.2 Conversation（对话）

```rust
pub struct Conversation {
    messages: Vec<Message>,
    source: Option<Source>,
}

pub struct Message {
    role: Role,
    content: String,
}

pub enum Role {
    Human,
    Assistant,
}
```

### 6.3 SearchQuery（搜索查询）

```rust
pub struct SearchQuery {
    pub text: String,
    pub filter: SearchFilter,
    pub pagination: Pagination,
}

pub struct SearchFilter {
    pub item_type: Option<ItemType>,
    pub tags: Vec<String>,
}
```

---

## 7. 数据流

### 7.1 知识提炼流程

```
┌────────────┐    ┌─────────────────┐    ┌───────────────┐    ┌──────────┐
│ 用户粘贴   │───▶│ Conversation    │───▶│ Extractor     │───▶│ Item[]   │
│ 对话文本   │    │ ::parse()       │    │ ::extract()   │    │          │
└────────────┘    └─────────────────┘    └───────────────┘    └──────────┘
                                               │
                                               ▼
                                         ┌───────────────┐
                                         │ LLM (Claude)  │
                                         │ 分析对话内容  │
                                         └───────────────┘
```

### 7.2 搜索流程

```
┌────────────┐    ┌─────────────────┐    ┌───────────────┐    ┌──────────┐
│ 用户输入   │───▶│ SearchQuery     │───▶│ SearchEngine  │───▶│ Results  │
│ 关键词     │    │ ::new()         │    │ ::search()    │    │          │
└────────────┘    └─────────────────┘    └───────────────┘    └──────────┘
                                               │
                                    ┌──────────┴──────────┐
                                    ▼                     ▼
                             ┌───────────┐         ┌───────────┐
                             │ FTS5 搜索 │         │ 向量搜索  │
                             │ (关键词)  │         │ (语义)    │
                             └───────────┘         └───────────┘
```

---

## 8. 技术栈

| 层级 | 技术 | 用途 |
|------|------|------|
| 核心库 | Rust | 类型安全、高性能 |
| 存储 | SQLite + FTS5 | 本地数据库 + 全文搜索 |
| 向量搜索 | (可选) | 语义相似度 |
| LLM | Claude / OpenAI | 知识提炼 |
| 桌面 | Tauri 2.0 | 跨平台桌面应用 |
| 前端 | React + TypeScript | UI |
| 插件 | Plasmo | 浏览器扩展 |

---

## 9. 扩展指南

### 9.1 新增 LLM Provider

在 `infra/llm.rs` 中添加新客户端：

```rust
pub struct GeminiClient {
    client: reqwest::Client,
    api_key: String,
}

#[async_trait]
impl LlmClient for GeminiClient {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> InfraResult<String> {
        // 实现 Gemini API 调用
    }
}
```

更新 `infra/mod.rs` 导出。

### 9.2 新增搜索后端

实现 `VectorSearch` trait：

```rust
pub struct QdrantSearch {
    // ...
}

#[async_trait]
impl VectorSearch for QdrantSearch {
    async fn search(&self, query: &str, limit: usize) -> InfraResult<Vec<(String, f32)>> {
        // 实现向量搜索
    }
    // ...
}
```

---

## 10. 代码质量检查

```bash
# 编译检查
cargo check --package refine-core

# 测试
cargo test --package refine-core

# Clippy
cargo clippy --package refine-core -- -D warnings

# 格式化
cargo fmt --package refine-core
```
