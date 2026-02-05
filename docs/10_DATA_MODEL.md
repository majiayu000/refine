# Refine - 数据模型规格

> 核心数据类型定义和关系

---

## 1. 核心实体

### 1.1 Item（知识条目）

知识库的核心实体，表示一个知识片段、技能或代码片段。

```rust
pub struct Item {
    id: ItemId,           // 唯一标识
    item_type: ItemType,  // 类型
    title: String,        // 标题
    summary: String,      // 摘要
    content: String,      // 完整内容
    tags: Vec<Tag>,       // 标签
    source: Option<Source>, // 来源
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}
```

#### 字段说明

| 字段 | 类型 | 说明 | 约束 |
|------|------|------|------|
| `id` | `ItemId` | UUID 格式唯一标识 | 自动生成 |
| `item_type` | `ItemType` | 条目类型 | 必填 |
| `title` | `String` | 标题 | 1-200 字符 |
| `summary` | `String` | 摘要描述 | 1-500 字符 |
| `content` | `String` | 完整内容 | 可选，无限制 |
| `tags` | `Vec<Tag>` | 标签列表 | 0-20 个 |
| `source` | `Option<Source>` | 来源信息 | 可选 |
| `created_at` | `DateTime<Utc>` | 创建时间 | 自动设置 |
| `updated_at` | `DateTime<Utc>` | 更新时间 | 自动更新 |

---

### 1.2 ItemType（条目类型）

```rust
pub enum ItemType {
    Knowledge,  // 知识概念
    Skill,      // 可执行技能（Prompt 模板）
    Snippet,    // 代码片段
}
```

| 类型 | 说明 | 用途 |
|------|------|------|
| `Knowledge` | 概念性知识 | 最佳实践、原理解释 |
| `Skill` | 可执行技能 | Prompt 模板，填参即用 |
| `Snippet` | 代码片段 | 可复用的代码块 |

---

### 1.3 ItemId（条目标识）

```rust
pub struct ItemId(Uuid);

impl ItemId {
    pub fn new() -> Self;           // 生成新 ID
    pub fn from_str(s: &str) -> Self; // 从字符串解析
    pub fn as_str(&self) -> &str;   // 转为字符串
}
```

---

### 1.4 Tag（标签）

```rust
pub struct Tag(String);

impl Tag {
    pub fn try_new(s: &str) -> Option<Self>; // 验证并创建
    pub fn as_str(&self) -> &str;
}
```

**验证规则**：
- 长度：1-50 字符
- 字符：字母、数字、中文、连字符、下划线
- 自动转小写

---

### 1.5 Source（来源）

```rust
pub struct Source {
    pub platform: String,           // 平台：chatgpt, claude
    pub conversation_id: Option<String>, // 对话 ID
    pub url: Option<String>,        // 原始 URL
}
```

---

## 2. 对话相关

### 2.1 Conversation（对话）

```rust
pub struct Conversation {
    messages: Vec<Message>,
    source: Option<Source>,
}

impl Conversation {
    pub fn parse(text: &str) -> Option<Self>; // 解析对话文本
    pub fn messages(&self) -> &[Message];
}
```

---

### 2.2 Message（消息）

```rust
pub struct Message {
    pub role: Role,
    pub content: String,
}

pub enum Role {
    Human,
    Assistant,
}
```

---

## 3. 搜索相关

### 3.1 SearchQuery（搜索查询）

```rust
pub struct SearchQuery {
    pub text: String,           // 搜索关键词
    pub filter: SearchFilter,   // 过滤条件
    pub pagination: Pagination, // 分页
}
```

---

### 3.2 SearchFilter（过滤条件）

```rust
pub struct SearchFilter {
    pub item_type: Option<ItemType>, // 类型过滤
    pub tags: Vec<String>,           // 标签过滤
}
```

---

### 3.3 Pagination（分页）

```rust
pub struct Pagination {
    pub offset: usize, // 偏移量
    pub limit: usize,  // 数量限制（默认 20，最大 100）
}
```

---

### 3.4 SearchResult（搜索结果）

```rust
pub struct SearchResult {
    pub items: Vec<SearchHit>,
    pub total: usize,
}

pub struct SearchHit {
    pub item: Item,
    pub score: f32, // 相关度分数
}
```

---

## 4. 数据库 Schema

### 4.1 items 表

```sql
CREATE TABLE items (
    id TEXT PRIMARY KEY,
    item_type TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    content TEXT NOT NULL,
    tags TEXT NOT NULL,       -- JSON 数组
    source TEXT,              -- JSON 对象
    created_at TEXT NOT NULL, -- ISO 8601
    updated_at TEXT NOT NULL  -- ISO 8601
);

CREATE INDEX idx_items_type ON items(item_type);
CREATE INDEX idx_items_created ON items(created_at);
```

---

### 4.2 items_fts 全文搜索表

```sql
CREATE VIRTUAL TABLE items_fts USING fts5(
    title,
    summary,
    content,
    tags,
    content=items,
    content_rowid=rowid
);
```

**触发器**：
- `items_ai` - 插入后同步 FTS
- `items_ad` - 删除后同步 FTS
- `items_au` - 更新后同步 FTS

---

## 5. 序列化格式

### 5.1 JSON 序列化示例

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "item_type": "knowledge",
  "title": "Rust 错误处理最佳实践",
  "summary": "使用 thiserror 定义库错误，anyhow 处理应用错误",
  "content": "详细内容...",
  "tags": ["rust", "error-handling"],
  "source": {
    "platform": "claude",
    "url": "https://claude.ai/chat/xxx"
  },
  "created_at": "2026-02-05T15:30:00Z",
  "updated_at": "2026-02-05T15:30:00Z"
}
```

---

## 6. 数据流

```
┌────────────┐    ┌────────────────┐    ┌────────────┐
│ 用户输入   │───▶│ Conversation   │───▶│ Extractor  │
│ (对话文本) │    │ ::parse()      │    │            │
└────────────┘    └────────────────┘    └─────┬──────┘
                                              │
                                              ▼
                                        ┌────────────┐
                                        │ Item[]     │
                                        │ (知识条目) │
                                        └─────┬──────┘
                                              │
                                              ▼
                                        ┌────────────┐
                                        │ SqliteStore│
                                        │ ::save()   │
                                        └─────┬──────┘
                                              │
                          ┌───────────────────┼───────────────────┐
                          ▼                   ▼                   ▼
                    ┌──────────┐        ┌──────────┐        ┌──────────┐
                    │ items    │        │items_fts │        │ (向量)   │
                    │ 表       │        │ FTS5     │        │ 未来    │
                    └──────────┘        └──────────┘        └──────────┘
```

---

## 7. 扩展点

### 7.1 新增 ItemType

1. 在 `ItemType` 枚举中添加新变体
2. 更新 `as_str()` 方法
3. 更新 `Item::new_xxx()` 工厂方法

### 7.2 新增字段

1. 在 `Item` struct 中添加字段
2. 更新 `schema.sql` 和迁移脚本
3. 更新 `SqliteStore` 的序列化/反序列化
