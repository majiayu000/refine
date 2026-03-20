# Refine

> 智能知识复用引擎 — 让每一次 AI 对话都成为可复用的资产

从 AI 对话中自动提炼知识，在需要时主动推荐。支持 Claude Code、Codex、ChatGPT 等对话来源。

## 核心功能

- **智能提炼** — 从 AI 对话中提取知识卡片、技能、代码片段
- **全文搜索** — SQLite FTS5 驱动的中英混合搜索
- **文档管理** — 原文存储 + 关联知识追溯
- **Session Insights** — 从 Claude Code / Codex 会话中提取认知洞察，追踪成长
- **多端支持** — CLI / 桌面应用 / 浏览器插件 / API 服务

## 快速开始

```bash
# 安装
cargo install --path apps/cli

# 配置 LLM（.env 文件，支持 OpenAI 兼容 API）
cat > .env << 'EOF'
REFINE_OPENAI_API_KEY=your_key
REFINE_OPENAI_BASE_URL=https://api.openai.com
REFINE_OPENAI_MODEL=gpt-4o
EOF

# 导入会话（扫描 ~/.claude/projects/ 和 ~/.codex/sessions/）
refine ingest-sessions

# 生成认知报告
refine insights --prescription

# 查看成长仪表盘
refine growth
```

## CLI 命令

### Session Insights（认知分析）

```bash
refine ingest-sessions                  # 导入全部会话（增量，自动跳过已处理的）
refine ingest-sessions --source claude  # 只导入 Claude Code
refine ingest-sessions --limit 100      # 限制数量
refine ingest-sessions --dry-run        # 预览，不调 LLM

refine insights                         # 生成 L1-L3 报告
refine insights --prescription          # 含 L4 成长处方

refine growth                           # 认知仪表盘
refine explore                          # 标记一次探索 session
refine deep-inquiry                     # 标记一次深度思考 session
```

### 知识管理

```bash
refine extract --stdin                  # 从标准输入提炼知识
refine search "query"                   # 搜索知识
refine list                             # 列出所有知识
refine list --type observation          # 列出认知观测
refine show <id>                        # 查看详情
refine docs                             # 列出会话文档
refine doc-show <id>                    # 查看会话/报告详情
```

## 认知仪表盘

`refine growth` 输出：

```
╔══════════════════════════════════════════════════════╗
║                    认知成长仪表盘                    ║
╠══════════════════════════════════════════════════════╣
║ 总会话: 824  总观测: 9740                            ║
╠══════════════════════════════════════════════════════╣
║ 认知水平分布                                         ║
║  expert      █░░░░░░░░░  11.5% ( 95)                ║
║  proficient  ███░░░░░░░  34.1% (281)                ║
║  competent   ████░░░░░░  39.8% (328)                ║
╠══════════════════════════════════════════════════════╣
║ 协作模式                                             ║
║  delegation  █████░░░░░  45.5% (375)                ║
║  deep_inq    ██░░░░░░░░  18.8% (155)                ║
║  exploration ██░░░░░░░░  16.5% (136)                ║
╠══════════════════════════════════════════════════════╣
║ 关键指标                                             ║
║  探索率           16.5%  目标: >15%   ✓              ║
║  delegation    45.5%  目标: <40%   ✗                 ║
║  expert率       11.5%  目标: >15%   ✗                ║
╚══════════════════════════════════════════════════════╝
```

## 架构

```
Claude Code / Codex 会话文件 (.jsonl)
    │
    ▼ refine ingest-sessions
    解析 → 过滤 → 12 维度 facet 提取 → SQLite
    （3 路并发，断点续传，指数退避重试）
    │
    ▼ refine insights
    本地聚类（按项目分组）→ 10 路并发 LLM 分析 → 合并报告
    │
    ▼ 三层持续追踪
    终端 motd | refine growth | Claude Code statusline
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
│   │   ├── discovery.rs     # 会话文件发现
│   │   ├── parser.rs        # JSONL 解析（Claude Code + Codex）
│   │   ├── facets.rs        # 12 维度 facet 提取
│   │   ├── clustering.rs    # 本地聚类（按项目分组）
│   │   ├── analysis_routes.rs # 10 路 LLM 分析任务
│   │   └── report.rs        # 报告合并
│   ├── search/          # 搜索引擎（FTS5）
│   └── infra/           # 基础设施（SQLite, LLM 客户端）
├── apps/
│   ├── cli/             # CLI 工具（refine 命令）
│   ├── server/          # API 服务（Axum）
│   └── desktop/         # 桌面应用（Tauri）
└── scripts/
    ├── weekly-insights.sh    # 每日自动分析（launchd）
    └── reset-weekly-tracker.sh # 周计数器重置
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
