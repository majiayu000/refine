<h1 align="center">Refine</h1>

<p align="center"><strong>Re + Fine — 持续精进，每一次对话都更好一点。</strong></p>

<p align="center">把 ChatGPT、Claude、Gemini、Grok、Claude Code、Codex 的对话知识统一同步到一个知识库。</p>

<p align="center"><a href="./README.md">English</a></p>

## 核心功能

`refine ingest-sessions` 要求 `PATH` 中存在兼容的 `remem`，也可通过
`REFINE_REMEM_BIN` 指定二进制路径。命令只读取 Remem raw archive；子进程、
JSON、合同或分页错误都会显式失败，公开 CLI 没有自动或显式的本地
transcript 回退。匹配的历史 Document/items 收敛与 Remem 引用投影保存会在
同一事务提交；身份不唯一时命令显式失败。

- **跨平台知识同步（主线）** — 把 ChatGPT、Claude、Gemini、Grok、Claude Code、Codex 的对话知识统一同步
- **会话存储与可追溯** — 保存 Remem 引用与提炼结果，需要时按引用读取原文
- **智能提炼（可选能力层）** — 从已同步对话中提取知识卡片、技能、代码片段
- **全文搜索** — SQLite FTS5 驱动的中英混合搜索
- **Session Insights 与成长分析（可选能力层）** — 对 Claude Code / Codex 会话做认知分析
- **多端访问** — 浏览器扩展 / API 服务 / CLI / 桌面应用

## Refine 主要是干嘛的

- **主流程**：先把多平台聊天知识同步到统一知识库
- **后续能力**：在同步数据上做搜索、提炼、推荐、洞察

## 文档导航

- [使用指南](./docs/USAGE.md)
- [项目总览](./docs/00_OVERVIEW.md)
- [服务端说明](./apps/server/README.md)
- [API 规格](./docs/11_API_SPEC.md)
- [Claude Hook 无感导入设计](./docs/13_CLAUDE_HOOK_INGESTION.md)

## 浏览器扩展示意

![Refine 浏览器扩展面板](docs/images/extension-dashboard.png)

## 快速开始

CLI 本地链路（优先）：

```bash
# 安装
cargo install --path apps/cli

# 配置 LLM（.env 文件，支持 OpenAI 兼容 API）
cat > .env << 'EOF'
REFINE_OPENAI_API_KEY=your_key
REFINE_OPENAI_BASE_URL=https://api.openai.com
REFINE_OPENAI_MODEL=gpt-4o
EOF

# 导入会话（从 remem raw archive 读取）
refine ingest-sessions

# 生成认知报告
refine insights --prescription

# 查看成长仪表盘
mirror dashboard
```

## CLI 命令

### Session Insights（认知分析）

```bash
refine ingest-sessions                  # 从 remem raw archive 导入会话投影
refine ingest-sessions --latest 20      # 从新到旧最多处理 20 个有效待处理会话
refine ingest-sessions --dry-run        # 预览，不调 LLM

refine insights                         # 生成 L1-L3 报告
refine insights --prescription          # 含 L4 成长处方

mirror dashboard                        # 认知成长仪表盘（替代已移除的 refine growth）
mirror score                            # 三层信号灯评分
```

`--latest N` 限制的是 Refine 最终待处理集合，不是 Remem 摘要窗口。Refine 会先读取
完整摘要集合做身份判定，再按时间从新到旧扫描；重复、低信号、显式 Looper 定时任务和
已隔离会话都不占 N。选满后不会再读取更旧会话正文。不传 `--latest` 时仍可手动处理
全历史。

### 知识管理

```bash
refine extract --stdin                  # 从标准输入提炼知识
refine search "query"                   # 搜索知识
refine list                             # 列出所有知识
refine list --type observation          # 列出认知观测
refine add --title "t" --summary "s" --type knowledge  # 添加知识
refine show <id>                        # 查看详情
refine delete <id>                      # 删除知识
refine docs                             # 列出会话文档
refine doc-show <id>                    # 查看会话/报告详情
refine doc-search "query"               # 搜索原文文档
```

## 认知仪表盘

使用 `mirror dashboard` 查看认知成长仪表盘（`refine growth` 已移除，请改用 `mirror dashboard`）。

## 架构

```
remem raw archive（精确 source_root/project/session_id tuple）
    │
    ▼ refine ingest-sessions
    契约校验 → 过滤 → 12 维度 facet 提取 → SQLite
    （默认串行，可配置并发，指数退避重试）
    │
    ▼ refine insights
    本地聚类（按项目分组）→ 10 路并发 LLM 分析 → 合并报告
    │
    ▼ 三层持续追踪
    终端 motd | mirror dashboard | 周报追踪脚本
```

### 提取的 12 个维度

| 维度 | 说明 |
|------|------|
| decisions | 技术决策与取舍理由 |
| bugs_fixed | bug 根因 + 修复方案 |
| patterns | 可复用的代码模式 |
| friction | AI 犯错、卡住、方向错误 |
| project_progress | 推进了什么 |
| questions | 提出的问题（反映知识边界）|
| knowledge_gained | 新学到的东西 |
| tools_discovered | 发现的新工具/库 |
| architecture | 架构设计与数据流 |
| code_artifacts | 关键代码产出 |
| cognitive_level | novice → expert（Dreyfus）|
| collaboration_mode | delegation / exploration / deep_inquiry / ... |

## 项目结构

```
refine/
├── packages/core/src/
│   ├── knowledge/       # 知识管理（Item, Document, Repository）
│   ├── refinement/      # 知识提炼（Conversation, Extractor）
│   ├── session/         # 会话分析
│   │   ├── discovery.rs     # 历史本地扫描实现（公开 CLI 不暴露）
│   │   ├── parser.rs        # 历史 JSONL 解析实现
│   │   ├── facets.rs        # 12 维度 facet 提取
│   │   ├── clustering.rs    # 本地聚类（按项目分组）
│   │   ├── analysis_routes.rs # 10 路 LLM 分析任务
│   │   └── report.rs        # 报告合并
│   ├── search/          # 搜索引擎（FTS5）
│   └── infra/           # 基础设施（SQLite, LLM 客户端）
├── apps/
│   ├── cli/             # CLI 工具（refine 命令）
│   ├── server/          # API 服务（Axum）
│   ├── desktop/         # 桌面应用（Tauri）
│   └── extension/       # 浏览器扩展（Plasmo）
└── scripts/
    └── weekly-insights.sh    # 每周自动分析（launchd/cron）
```

## 技术栈

| 层 | 技术 |
|----|------|
| 核心库 | Rust |
| 数据库 | SQLite + FTS5 |
| LLM | OpenAI 兼容 API（支持自定义 base_url）|
| 桌面应用 | Tauri 2.0 |
| 浏览器插件 | Plasmo |

## License

MIT
