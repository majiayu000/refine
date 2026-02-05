# Refine

> 智能知识复用引擎 - 让每一次 AI 对话都成为可复用的资产

从 ChatGPT、Claude 等 AI 对话中自动提炼知识，在需要时主动推荐。

## 核心功能

- **自动采集** - 浏览器插件一键保存 AI 对话
- **智能提炼** - 自动生成知识卡片、技能、代码片段
- **主动推荐** - 问问题时自动匹配相关历史知识
- **可执行技能** - Prompt 模板化，填参即用
- **本地优先** - 数据存储在本地，隐私安全

## 快速开始

### 安装依赖

```bash
# Rust
cargo build --workspace

# 前端
cd apps/desktop/ui && bun install
cd apps/extension && bun install
```

### 运行

```bash
# CLI
cargo run --package refine-cli -- --help

# 桌面应用
cd apps/desktop/src-tauri && cargo tauri dev

# 浏览器扩展
cd apps/extension && bun dev
```

## 项目结构

```
refine/
├── packages/
│   └── core/             # Rust 核心库
│       └── src/
│           ├── knowledge/    # 知识管理
│           ├── refinement/   # 知识提炼
│           ├── search/       # 搜索引擎
│           └── infra/        # 基础设施
├── apps/
│   ├── cli/              # CLI 工具
│   ├── desktop/          # Tauri 桌面应用
│   │   ├── src-tauri/    # Rust 后端
│   │   └── ui/           # React 前端
│   └── extension/        # 浏览器插件 (Plasmo)
└── docs/                 # 文档
```

## CLI 命令

```bash
refine extract --stdin    # 从标准输入提取对话
refine search "query"     # 搜索知识
refine list               # 列出所有知识
refine show <id>          # 查看详情
refine delete <id>        # 删除知识
refine add --title "..." --summary "..."  # 添加知识
```

## 技术栈

| 层 | 技术 |
|----|------|
| 核心库 | Rust |
| 桌面应用 | Tauri 2.0 (Rust + React) |
| 浏览器插件 | Plasmo (React + TypeScript) |
| 数据库 | SQLite + FTS5 |
| 前端 | React 18 + Zustand + Tailwind CSS |

## 文档

### 产品

| 文档 | 说明 |
|-----|------|
| [项目总览](./docs/00_OVERVIEW.md) | 项目概述和导航 |
| [PRD](./docs/01_PRD.md) | 产品需求文档 |
| [用户研究](./docs/02_USER_RESEARCH.md) | 用户画像和痛点 |
| [优先级排序](./docs/03_RICE_PRIORITIZATION.md) | RICE 功能优先级 |
| [竞品分析](./docs/04_COMPETITIVE_ANALYSIS.md) | 市场竞争分析 |
| [GTM 策略](./docs/05_GTM_STRATEGY.md) | 上市策略 |

### 设计

| 文档 | 说明 |
|-----|------|
| [设计系统](./docs/06_DESIGN_SYSTEM.md) | UI 设计 Tokens 和组件 |

### 技术

| 文档 | 说明 |
|-----|------|
| [Rust 规范](./docs/07_RUST_GUIDELINES.md) | Rust 编码规范 |
| [React 规范](./docs/08_REACT_GUIDELINES.md) | React 最佳实践 |
| [架构设计](./docs/09_ARCHITECTURE.md) | 模块化架构 |
| [数据模型](./docs/10_DATA_MODEL.md) | 数据类型定义 |
| [API 规格](./docs/11_API_SPEC.md) | Tauri/HTTP API |
| [测试策略](./docs/12_TESTING.md) | 测试规范 |

## 开发路线

- **Q1**: MVP - 核心引擎 + 桌面应用 ✅
- **Q2**: v1.0 - 浏览器插件 + 技能系统 (进行中)
- **Q3**: v1.1 - 自动推荐 + CLI
- **Q4**: v1.2 - 云同步 + 技能市场

## License

MIT
