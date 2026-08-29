# Remem 单一原始会话源与 Refine 派生投影

## 责任边界

三个概念必须分开：

| 概念 | 回答的问题 | 示例 |
| --- | --- | --- |
| host/runtime | 对话由哪个工具产生 | `codex-cli`、`claude-code`、`cursor` |
| 原始事实源 | 谁拥有完整会话和稳定身份 | Remem |
| LLM 执行器 | 谁为 Refine 抽取 facets | Refine 当前配置的模型/API |

Remem 负责会话发现、原文、身份、host、项目、事件时间、消息数量和内容指纹。
Refine 负责认知 facet、Observation、聚合、日报、周报和画像。

## 选择

采用“Remem 原文 + Refine 引用投影”。不采用以下方案：

- 双份原文：重复 SQLite、FTS、备份和增量成本；
- Refine 直读 Remem DB/Rust crate：耦合 SQLCipher、schema 和迁移所有权；
- 立刻合并仓库：把记忆运行时与认知分析产品无必要地绑在一起。

## 稳定合同

Remem 会话摘要至少提供：

```json
{
  "session_ref": "remem://raw-session/v2/...",
  "host": "codex-cli",
  "source_root": "local",
  "project": "/repo",
  "session_id": "...",
  "first_epoch": 0,
  "last_epoch": 0,
  "message_count": 0,
  "content_hash": "sha256:..."
}
```

`source_root` 表示存储位置，不表示平台。`host` 只能由 Remem 的可信采集边界
确定。完整消息使用包含 host 的精确身份，不能按标题、项目 basename 或 UUID 猜测。

## Refine 持久化合同

会话文档持久化：

- `session_ref`；
- host 和项目；
- captured_at；
- content_hash；
- 关联的 Observation、标签和必要派生摘要。

会话文档不持久化：

- 完整用户 prompt；
- 完整助手输出；
- 拼接后的 `raw_content`；
- 会话原文的第二份 FTS。

报告和人工知识文档仍保留内联正文。会话 `doc-show` 按需调用 Remem；Remem
不可用时明确失败，已有 Observation 仍可读。

## 日常增量

1. 向 Remem 请求有界摘要；
2. 比较 session_ref 和 content_hash；
3. 未变化时不拉消息、不调用 LLM；
4. 新增或变化时分页读取消息，校验每页快照 hash 与摘要一致，再在内存中重建会话并抽取；
5. 事务保存引用和 Observation，随后丢弃原文。

## 不变量

- 完整会话只由 Remem 持久化；
- Refine 正常运行不直接扫描 host 会话目录；
- host 由 Remem 给出，Refine 不猜；
- 未变化 content_hash 不触发全文读取或 LLM；
- 摘要与分页快照 hash 不一致时显式失败，不保存混合版本；
- Remem 失败不触发第二事实源 fallback；
- 日报按 host 聚合，不按采集路径聚合。

本轮不统一 Remem memory extraction 与 Refine cognitive facets，也不新增 Grok、
Gemini 会话源。
