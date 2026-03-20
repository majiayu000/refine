# SPEC: Session Insights — 从 AI 编程对话中提取宏观认知洞察

## Goal

从本地 Claude Code 和 Codex 聊天记录中，自动提取结构化的认知观测（facets），并聚合生成能力提升处方。

## Context

### 数据源

| 来源 | 位置 | 格式 |
|------|------|------|
| Claude Code 会话 | `~/.claude/projects/*/*.jsonl` | 完整对话（含 AI 回复、tool use） |
| Codex 会话 | `~/.codex/sessions/YYYY/MM/DD/*.jsonl` | 完整对话（session_meta + response_item） |

`~/.claude/history.jsonl` 只有用户输入摘要，不含 AI 回复，不适合深度分析。会话级 JSONL 包含完整交互，是主要数据源。

### 现有架构

- Document = 原文一等公民（raw_content + FTS）
- Item = LLM 提取的视图/切片（knowledge/skill/snippet）
- CLI 通过 `Commands` 枚举分发，`handlers.rs` 实现
- LLM 调用通过 `LlmClient` trait（Claude/OpenAI 自动选择）

## Design

### 两个 CLI 命令

```
refine ingest-sessions [--source claude|codex|all] [--limit N] [--dry-run]
refine insights [--period 30d] [--format terminal|json]
```

### 数据流

```
Phase 1: 发现与解析                    Phase 2: Facet 提取
───────────────────                   ──────────────────
扫描 ~/.claude/ ~/.codex/              每个会话 → LLM 提取 10 维 facets
过滤子 agent、太短的会话                存为 Document(source="claude-code-session")
解析 JSONL → 统一 Session 结构          + Item(type=Observation) × N
跳过已处理的（按 Document.url 去重）     缓存：已处理的 session 不再重复

Phase 3: 聚合分析                      Phase 4: 处方生成
──────────────────                    ──────────────────
查询所有 Observation Items              技能发展路线图
按 L1/L2/L3 维度计算指标               学习策略优化建议
趋势分析（按周/月聚合）                 时间/注意力重分配
                                      AI 协作策略调优
                                      季度 OKR 自动生成
```

### 新增数据模型

#### ItemType 扩展

```rust
pub enum ItemType {
    Knowledge,
    Skill,
    Snippet,
    Observation,  // 新增：会话观测
}
```

#### Session 类型（新模块）

```rust
// 统一的会话结构（跨 Claude Code / Codex）
pub struct Session {
    pub id: String,                    // session UUID
    pub source: SessionSource,         // ClaudeCode | Codex
    pub project: Option<String>,       // 项目路径
    pub file_path: String,             // 原始 JSONL 文件路径
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub messages: Vec<SessionMessage>,
    pub metadata: SessionMeta,
}

pub enum SessionSource {
    ClaudeCode,
    Codex,
}

pub struct SessionMessage {
    pub role: MessageRole,             // User | Assistant | System | Tool
    pub content: String,               // 文本内容（tool_use 展开为描述）
    pub timestamp: Option<DateTime<Utc>>,
}

pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

pub struct SessionMeta {
    pub message_count: usize,
    pub user_message_count: usize,
    pub duration_minutes: Option<f64>,
    pub tools_used: Vec<String>,       // 使用的工具列表
    pub languages: Vec<String>,        // 涉及的编程语言
    pub git_branch: Option<String>,
    pub char_count: usize,             // 总字符数
}
```

### JSONL 解析规则

#### Claude Code 会话 JSONL

每行一个 JSON 对象，关键类型：
- `type: "summary"` — 会话摘要（可选）
- 无 type 但有 `message.role` — 对话消息
- `type: "progress"` — 工具执行进度（跳过）
- `type: "queue-operation"` — 队列操作（跳过）

提取用户消息（`role: "human"`）和助手消息（`role: "assistant"`）的文本内容。

#### Codex 会话 JSONL

每行一个 JSON 对象：
- `type: "session_meta"` — 会话元数据（cwd, model_provider 等）
- `type: "response_item"` + `payload.role: "user"` — 用户消息
- `type: "response_item"` + `payload.role: "assistant"` — 助手消息

### 过滤规则（参考 /insights）

跳过以下会话：
1. 子 agent 会话（文件名以 `agent-` 开头）
2. 用户消息 < 2 条
3. 总字符数 < 500
4. 时长 < 1 分钟

### Facet 提取 Prompt

对每个会话，用 LLM 提取以下 10 个维度的结构化 JSON：

```json
{
  "decisions": [{"what": "...", "why": "...", "alternatives": "..."}],
  "bugs_fixed": [{"symptom": "...", "root_cause": "...", "fix": "..."}],
  "patterns": [{"name": "...", "description": "...", "reusable": true}],
  "friction": [{"type": "...", "detail": "...", "who": "user|ai"}],
  "project_progress": "一句话描述推进了什么",
  "questions": [{"question": "...", "domain": "...", "depth": "syntax|usage|tradeoff|design"}],
  "knowledge_gained": [{"topic": "...", "insight": "..."}],
  "tools_discovered": [{"name": "...", "purpose": "..."}],
  "architecture": [{"component": "...", "decision": "..."}],
  "collaboration_mode": "delegate|iterative|interrogate|conceptual|mixed",
  "cognitive_level": "apply|analyze|evaluate|create",
  "session_summary": "2-3 句话摘要"
}
```

每个 facet 维度存为一个 Observation Item，关联到 Session Document。

### 聚合分析维度

#### L1 认知演进
- **Dreyfus 迁移**：按 `domain` 分组，追踪 `depth` 从 syntax→design 的变化
- **Bloom 认知层级**：`cognitive_level` 的分布趋势
- **双环学习**：`decisions` 中包含"质疑假设"类内容的比例

#### L2 战略定位
- **技术雷达**：`tools_discovered` + `questions.domain` 按频率/时间构建四环
- **探索/利用比**：新领域 vs 已知领域的会话比例
- **知识网络**：`knowledge_gained.topic` 的共现关系

#### L3 协作效能
- **协作模式分布**：`collaboration_mode` 的频率
- **摩擦分析**：`friction` 按 type 和 who 聚合
- **心流率**：会话持续时长 > 30min 的比例

#### L4 处方生成
基于 L1-L3 指标，用 LLM 生成：
1. 技能发展路线图（深耕/突破/放弃/盲区四象限）
2. 学习策略诊断与处方
3. 时间分配建议
4. AI 协作调优建议
5. 季度 OKR（可量化、下一轮可验证）

### 分块策略

- 会话内容 > 30K 字符时，按 25K 分块先摘要再提取
- 摘要使用轻量 prompt，保留关键对话转折和决策点
- 摘要后拼接送入 facet 提取 prompt

### 去重与增量

- 每个 Session 对应一个 Document，`url` 字段存 JSONL 文件路径
- `ingest-sessions` 执行前查询已有 Document.url，跳过已处理的
- 重新处理需要先删除对应 Document（级联删除关联 Items）

## Files Changed

### 新增文件（packages/core/src/session/）

| 文件 | 职责 |
|------|------|
| `mod.rs` | 模块导出 |
| `types.rs` | Session, SessionMessage, SessionMeta, SessionSource 类型 |
| `discovery.rs` | 扫描 ~/.claude/ ~/.codex/ 发现会话文件 |
| `parser.rs` | JSONL 解析器（Claude Code + Codex 两种格式） |
| `filter.rs` | 过滤规则（太短、子 agent 等） |
| `facets.rs` | Facet 提取 prompt 构建 + 响应解析 |
| `aggregation.rs` | L1-L3 聚合计算 |
| `prescription.rs` | L4 处方生成 prompt + 解析 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `packages/core/src/lib.rs` | 添加 `pub mod session;` |
| `packages/core/src/knowledge/types.rs` | ItemType 添加 `Observation` |
| `packages/core/src/infra/sqlite/ops.rs` | item_type 映射新增 Observation |
| `packages/core/src/infra/sqlite/rows.rs` | row_to_item 支持 Observation |
| `apps/cli/src/cli.rs` | Commands 添加 IngestSessions、Insights |
| `apps/cli/src/handlers.rs` | 添加 handle_ingest_sessions、handle_insights |

### 总计：8 个新文件 + 6 个修改文件 = 14 个文件

## Implementation Phases

### 阶段 1：基础设施（Session 解析）
1. `session/types.rs` — 数据类型
2. `session/discovery.rs` — 文件发现
3. `session/parser.rs` — JSONL 解析（两种格式）
4. `session/filter.rs` — 过滤
5. `session/mod.rs` — 导出
6. `packages/core/src/lib.rs` — 添加模块

验证：`cargo check` + 单元测试（解析样本 JSONL）

### 阶段 2：Facet 提取管道
1. `session/facets.rs` — prompt + 解析
2. `knowledge/types.rs` — ItemType::Observation
3. SQLite 映射更新（ops.rs, rows.rs）
4. `apps/cli/src/cli.rs` — IngestSessions 命令
5. `apps/cli/src/handlers.rs` — handle_ingest_sessions

验证：`cargo check` + `cargo test` + 手动测试 3 个真实会话

### 阶段 3：聚合与处方
1. `session/aggregation.rs` — L1-L3 指标计算
2. `session/prescription.rs` — L4 处方生成
3. `apps/cli/src/cli.rs` — Insights 命令
4. `apps/cli/src/handlers.rs` — handle_insights

验证：全量测试 + 手动运行 `refine insights`

## Constraints

- LLM 调用使用现有 `LlmClient` trait，不引入新依赖
- Facet 提取使用 Haiku/Sonnet 级别模型（成本控制）
- 处方生成使用 Opus 级别模型（需要深度推理）
- 单文件不超过 200 行
- 不修改 Server 端代码（这是 CLI only 功能）
- 增量处理：每次只处理新会话，不重复处理

## Done-when

1. `refine ingest-sessions` 能扫描、解析、提取 Claude Code + Codex 会话
2. `refine insights` 能输出 L1-L4 四层分析报告
3. 增量处理正常工作（第二次运行只处理新会话）
4. `cargo check` + `cargo test` 通过
5. 至少 3 个真实会话的 facet 提取结果质量可接受
