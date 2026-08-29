# Refine × Remem 单一会话事实源

- 决定日期：2026-08-29
- 实施日期：2026-08-30
- 核心决定：Remem 保存唯一完整会话，Refine 只保存稳定引用和派生认知结果

## 目标数据流

```text
Codex CLI / Claude Code / Cursor
                 ↓
               Remem
       唯一原始会话事实源
                 ↓
               Refine
     session 引用 + Observation
                 ↓
       日报 / 周报 / 认知画像
```

Refine 的正常 `ingest-sessions` 不再扫描 `~/.codex/sessions` 或
`~/.claude/projects`，也不在自己的 SQLite 中保存第二份完整会话。日报展示
`codex-cli / claude-code / cursor`，而不是把 `local`、`remem` 误当成平台。

详细设计见：

- [01 单一事实源架构](./01-remem-single-source-architecture.md)
- [02 实施、迁移和真实性验收](./02-implementation-migration-and-proof.md)

## 不接受的伪修复

- 把所有未知来源直接改名为 Codex；
- 只修报告标签，继续保存两份原文；
- Remem 失败时静默回退 Refine 直采；
- 为修来源身份而重跑 90 天 LLM；
- 只以单元测试、PR 合并或截图宣称真实环境已修复。
