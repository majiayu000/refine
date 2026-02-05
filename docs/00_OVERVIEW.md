# Refine - 项目总览

> 智能知识复用引擎 - 让每一次 AI 对话都成为可复用的资产

---

## 项目简介

**Refine** 是一个专为 AI 对话设计的知识管理工具，自动从与 ChatGPT、Claude 等 AI 的对话中提炼知识片段和可执行技能，在用户需要时主动推荐，消除重复提问，放大 AI 对话的价值。

---

## 文档索引

### 产品文档

| 文档 | 说明 | 状态 |
|-----|------|------|
| [01_PRD.md](./01_PRD.md) | 产品需求文档 | ✅ |
| [02_USER_RESEARCH.md](./02_USER_RESEARCH.md) | 用户画像和痛点 | ✅ |
| [03_RICE_PRIORITIZATION.md](./03_RICE_PRIORITIZATION.md) | RICE 功能优先级 | ✅ |
| [04_COMPETITIVE_ANALYSIS.md](./04_COMPETITIVE_ANALYSIS.md) | 竞品分析 | ✅ |
| [05_GTM_STRATEGY.md](./05_GTM_STRATEGY.md) | 上市策略 | ✅ |

### 设计文档

| 文档 | 说明 | 状态 |
|-----|------|------|
| [06_DESIGN_SYSTEM.md](./06_DESIGN_SYSTEM.md) | UI 设计系统 (Tokens/组件/布局) | ✅ |

### 技术文档

| 文档 | 说明 | 状态 |
|-----|------|------|
| [07_RUST_GUIDELINES.md](./07_RUST_GUIDELINES.md) | Rust 核心库编码规范 | ✅ |
| [08_REACT_GUIDELINES.md](./08_REACT_GUIDELINES.md) | React/Tauri 前端规范 | ✅ |
| [09_ARCHITECTURE.md](./09_ARCHITECTURE.md) | 模块化架构设计 | ✅ |
| [10_DATA_MODEL.md](./10_DATA_MODEL.md) | 数据模型规格 | ✅ |
| [11_API_SPEC.md](./11_API_SPEC.md) | Tauri/HTTP API 规格 | ✅ |
| [12_TESTING.md](./12_TESTING.md) | 测试策略 | ✅ |
| [TECH_STACK.md](../TECH_STACK.md) | 技术选型 | ✅ |
| [PRODUCT.md](../PRODUCT.md) | 产品形态设计 | ✅ |

### 代码实现

| 文件 | 说明 | 状态 |
|-----|------|------|
| [Spotlight.tsx](../apps/desktop/ui/src/components/spotlight/Spotlight.tsx) | 全局搜索组件 | ✅ |

---

## 文档使用的 Skills

本项目文档使用以下 Claude Skills 生成：

| Skill | 用途 | 产出文档 |
|-------|------|---------|
| **product-manager** | 产品管理框架 | PRD、用户研究、RICE、竞品、GTM |
| **ui-designer** | 设计系统生成 | 设计 Tokens、组件规范 |
| **rust-best-practices** | Rust 编码规范 | 错误处理、API 设计、性能 |
| **react-best-practices** | React 性能优化 | Bundle/渲染/状态管理 |
| **frontend-design** | 前端界面设计 | Spotlight 组件实现 |

---

## 核心架构

```
┌─────────────────────────────────────────────────────────────┐
│                        采集层                                │
│     浏览器插件 │ 手动粘贴 │ API Hook │ 剪贴板监听            │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                        处理层                                │
│              智能提炼 → 自动分类 → 向量化                     │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                        存储层                                │
│              SQLite + sqlite-vss │ 本地优先                  │
└──────────────────────────┬──────────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                        消费层                                │
│          桌面应用 │ 浏览器插件 │ CLI │ 全局搜索              │
└─────────────────────────────────────────────────────────────┘
```

---

## 技术栈

| 层 | 技术 | 说明 |
|----|------|------|
| 桌面应用 | Tauri 2.0 | Rust 后端 + React 前端 |
| 浏览器插件 | Plasmo | React + TypeScript |
| 核心库 | Rust | 提炼/存储/搜索 |
| 数据库 | SQLite + vss | 本地向量搜索 |
| 前端框架 | React 18 | + Zustand + React Query |
| 样式 | Tailwind CSS | + CVA |

---

## 里程碑

| 阶段 | 内容 | 目标 |
|-----|------|------|
| **Q1: MVP** | 核心引擎 + 桌面应用 | 验证价值 |
| **Q2: v1.0** | 浏览器插件 + 技能系统 | 1000 用户 |
| **Q3: v1.1** | 自动推荐 + CLI | 5000 用户 |
| **Q4: v1.2** | 云同步 + 技能市场 | 10000 用户 |

---

## 下一步

### 已完成

- [x] Rust/Tauri 项目结构
- [x] 核心数据模型 (Item, Conversation, SearchQuery)
- [x] SQLite 存储层 + FTS5 全文搜索
- [x] LLM 客户端 (Claude/OpenAI)
- [x] 知识提炼器 (Extractor)
- [x] 搜索引擎 (SearchEngine)
- [x] CLI 命令实现 (extract, search, list, show, delete, add)
- [x] Tauri 桌面应用 + Spotlight 组件
- [x] 浏览器插件 (Plasmo, ChatGPT/Claude 支持)
- [x] 扩展与桌面应用 HTTP API 通信

### 后续计划

1. 实现 LLM 驱动的智能提炼
2. 添加更多 AI 平台支持 (Gemini, Copilot)
3. 实现向量搜索 (语义相似度)
4. 添加知识推荐功能
5. 云同步功能
