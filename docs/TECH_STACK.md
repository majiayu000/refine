# Refine 技术选型

## 整体技术栈

```
┌─────────────────────────────────────────────────────────────┐
│                        前端 / 客户端                         │
├─────────────────────────────────────────────────────────────┤
│  桌面应用: Tauri (Rust + Web)                               │
│  浏览器插件: Plasmo (React + TypeScript)                    │
│  CLI: Rust (clap)                                          │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                         核心服务                             │
├─────────────────────────────────────────────────────────────┤
│  语言: Rust                                                 │
│  数据库: SQLite + sqlite-vss (向量搜索)                      │
│  LLM 调用: OpenAI API / Claude API / 本地 Ollama            │
│  Embedding: text-embedding-3-small / local model           │
└─────────────────────────────────────────────────────────────┘
```

---

## 各组件技术选型

### 桌面应用

| 选项 | 优点 | 缺点 | 结论 |
|-----|------|------|------|
| **Tauri** | 体积小 (~10MB)、性能好、Rust 后端 | 生态较新 | ✅ 推荐 |
| Electron | 生态成熟、开发快 | 体积大 (~150MB)、内存占用高 | ❌ |
| Flutter | 跨平台、性能好 | Dart 语言、桌面支持一般 | ❌ |

**选择: Tauri 2.0**
- 前端: React + TypeScript + Tailwind
- 后端: Rust (共享核心逻辑)

### 浏览器插件

| 选项 | 优点 | 缺点 | 结论 |
|-----|------|------|------|
| **Plasmo** | React 开发、自动热更新、跨浏览器 | 框架依赖 | ✅ 推荐 |
| WXT | 轻量、Vue/React 都支持 | 较新 | 备选 |
| 原生 | 无依赖 | 开发效率低 | ❌ |

**选择: Plasmo**
- 支持 Chrome、Firefox、Edge
- 与桌面应用共享 UI 组件

### CLI

| 选项 | 优点 | 缺点 | 结论 |
|-----|------|------|------|
| **Rust (clap)** | 性能好、与核心共享代码 | 编译慢 | ✅ 推荐 |
| Go (cobra) | 编译快、单文件分发 | 需要维护两套代码 | 备选 |
| Node.js | 开发快 | 运行时依赖 | ❌ |

**选择: Rust + clap**
- 与 Tauri 后端共享核心库
- 单文件分发

### 数据库

| 选项 | 优点 | 缺点 | 结论 |
|-----|------|------|------|
| **SQLite + sqlite-vss** | 本地优先、向量搜索、无需额外服务 | 向量搜索性能一般 | ✅ 推荐 |
| SQLite + faiss | 向量搜索性能好 | faiss 编译复杂 | 备选 |
| PostgreSQL + pgvector | 功能强大 | 需要运行数据库服务 | ❌ |

**选择: SQLite + sqlite-vss**
- 零配置、本地运行
- 数据量 <100k 条足够使用

### LLM 调用

**提炼用 LLM:**
- 主要: Claude API (claude-3-haiku，便宜快速)
- 备选: OpenAI API (gpt-4o-mini)
- 本地: Ollama (llama3.2)

**Embedding:**
- 主要: OpenAI text-embedding-3-small (便宜、效果好)
- 本地: nomic-embed-text (Ollama)

---

## 项目结构

```
refine/
├── apps/
│   ├── desktop/              # Tauri 桌面应用
│   │   ├── src/              # Rust 后端
│   │   ├── src-tauri/        # Tauri 配置
│   │   └── ui/               # React 前端
│   ├── extension/            # 浏览器插件 (Plasmo)
│   │   ├── background.ts
│   │   ├── content.ts
│   │   └── popup/
│   └── cli/                  # CLI 工具
│       └── src/
├── packages/
│   ├── core/                 # 核心库 (Rust)
│   │   ├── src/
│   │   │   ├── extractor.rs  # 知识提取
│   │   │   ├── storage.rs    # 数据存储
│   │   │   ├── search.rs     # 向量搜索
│   │   │   └── llm.rs        # LLM 调用
│   │   └── Cargo.toml
│   └── ui/                   # 共享 UI 组件
│       ├── components/
│       └── package.json
├── Cargo.toml                # Rust workspace
├── package.json              # Node.js workspace
└── turbo.json                # Monorepo 构建
```

---

## 核心数据模型

```sql
-- 知识片段表
CREATE TABLE items (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,           -- 'knowledge' | 'skill' | 'snippet'
    title TEXT NOT NULL,
    content TEXT NOT NULL,        -- JSON 格式的完整内容
    tags TEXT,                    -- JSON 数组
    source_platform TEXT,         -- 'chatgpt' | 'claude' | 'gemini' | 'manual'
    source_conversation_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- 向量索引表 (sqlite-vss)
CREATE VIRTUAL TABLE item_embeddings USING vss0(
    embedding(1536)               -- OpenAI embedding 维度
);

-- 原始对话表
CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL,
    conversation_id TEXT,
    content TEXT NOT NULL,        -- 原始对话 JSON
    created_at INTEGER NOT NULL
);

-- 索引
CREATE INDEX idx_items_type ON items(type);
CREATE INDEX idx_items_created ON items(created_at);
CREATE INDEX idx_items_tags ON items(tags);
```

---

## 开发路线

### Phase 1: MVP (核心验证)

- [ ] 核心库: 知识提取 + 存储 + 搜索
- [ ] CLI: 基础 CRUD 命令
- [ ] 手动添加知识片段

### Phase 2: 桌面应用

- [ ] Tauri 应用框架搭建
- [ ] 知识库浏览界面
- [ ] 全局搜索悬浮窗
- [ ] 技能编辑器

### Phase 3: 浏览器插件

- [ ] ChatGPT/Claude 页面适配
- [ ] 对话保存功能
- [ ] 侧边栏推荐
- [ ] 实时输入匹配

### Phase 4: 高级功能

- [ ] 云同步
- [ ] 团队共享
- [ ] 更多平台支持
