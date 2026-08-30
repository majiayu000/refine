# 实施、迁移和真实性验收

## 已复现的问题

固定窗口 `[2026-08-21T14:45:03Z, 2026-08-28T14:45:03Z)` 中，Refine
把 88 个 Remem 会话、752 条 Observation 归为 `platform_unknown`。独立按真实
会话路径核对后，其中 Codex 为 73 个会话/605 条 Observation，Claude Code 为
15/147；加上旧直采 Claude 数据后，正确口径是 Codex 73/605、Claude 27/255，
不是 Codex 0。

另一个运行问题是无人值守任务在 provenance 步骤失败后仍继续：它枚举 8287
个 Remem 会话并运行超过 16 小时。该任务已在迁移前受控停止，日志保留。

这些数字只用于复现和迁移 oracle，不能硬编码进产品。

## 实施阶段

### P0 Remem 合同

- 输出可信 host、session_mode、稳定 session_ref、content_hash，并让分页消息返回冻结快照 hash；
- 会话身份隔离相同 ID 的不同 host；
- Codex 的 TUI/Desktop、codex_exec/Symphony、subagent 分别映射 interactive、
  unattended、subagent；未知来源保持 unknown，已知模式冲突时写入前失败；
- Refine 枚举时读取完整摘要集合，不把 `--latest` 下推给 Remem，以完整判定 legacy
  identity uniqueness；
- provenance 缺失或冲突时 fail closed。

### P0 Refine 引用投影

- 正常 `ingest-sessions` 只读 Remem，摘要与消息快照不一致时中止；
- 摘要以 `content_hash + session_mode` 判断投影是否变化，模式修正不会被旧 hash
  快路径吞掉；
- 新抽取与无 LLM 的 legacy 收敛都会把可信 mode 原子写入 Observation 标签；
- 完整摘要只携带首条用户消息的有界 sample，用于在全文拉取前识别 Looper；普通
  unchanged 会话仍不下载全文；
- 抽取时原文只存在内存；
- 保存空会话正文、Remem 引用、host、hash 和 Observation；
- `doc-show` 按引用即时取原文。

### P0 无 LLM 历史收敛

历史文档按 `exact / missing / ambiguous` 处理。exact 记录保留 canonical
Document ID；obsolete duplicate Document 允许删除，但其中的所有 Item 必须保留原
ID、内容和标签，并在同一事务重挂到 canonical Document。迁移不按内容去重。
missing 需要正常的新会话抽取；ambiguous 明确报错，二者都不猜测。

纯迁移必须在没有 LLM 凭据时成功，并满足：

- canonical Document ID 不变；obsolete duplicate Document ID 允许删除；
- Observation 数、ID 和内容 hash 集合不变；
- LLM 调用为 0；
- host 数与独立 oracle 一致。

不做 90 天全量重跑。只计算 `Remem session_ref - Refine 已有有效 Observation`
集合，并对真正 missing 的有界列表做后续抽取。

### P1 无人值守合同

- Remem/摄入失败后不运行 mirror、周报或画像；
- `last-refresh-ok` 只代表整条链路成功；
- 定时运行默认传 `--latest 80`；该上限约束最终 eligible pending 集合，重复、低信号、
  显式 Looper 定时任务和隔离记录不占额度，选满后不再读取更旧正文；
- 命中的 Looper 会话原子清空 stable Document 的旧 Items，并删除匹配的 legacy
  Document/Items；失败时整组回滚，无关文档不受影响；
- weekly LaunchAgent wrapper 显式提供 Homebrew、系统和 Cargo bin PATH，launchd
  极简环境下仍能解析 Remem；
- 同一时刻只有一个 runtime job。

## 五层真实性门禁

1. 代码：三 host、碰撞、未知 host、hash 稳定、失败阻断测试。
2. 合同：直接检查真实 `remem raw sessions/messages --json`。
3. 数据：独立 oracle 满足 `oracle.host == remem.host == refine.host == report.host`。
4. 运行时：记录 commit、binary SHA-256、安装路径、实际进程和版本。
5. 用户结果：固定 cutoff collector 加一份新 Codex/Claude 真实会话 smoke。

最终 proof bundle 只保存身份、哈希、数量和命令结果，不保存聊天正文：

```text
proof/source-provenance-YYYYMMDD/
├── manifest.json
├── oracle.tsv
├── remem-contract.json
├── refine-before.json
├── refine-after.json
├── conservation.json
├── no-llm-migration.json
├── failure-injection.json
├── runtime-binaries.json
├── live-smoke.json
└── verdict.json
```

该 proof bundle 是代码合并后，使用实际安装的 Remem/Refine binary、真实 runtime
和新增真实会话执行的关闭门禁。本 PR 只提供可重复的代码与隔离测试证据，不生成、
替代或伪造上述 live evidence。

只有代码、合同、历史数据、安装运行时和最终报告全部通过，才允许关闭 Issue。
