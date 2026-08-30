# Cognitive Portrait — 历史归档索引

> 长期认知画像归档目录。v0-v3 是历史全量快照；v4 默认记录 current/previous 90d 可比窗口。
> 详细架构和演进路线见 [SPEC.md](./SPEC.md)。

## 报告清单

| date | version | sessions | total_lines | L1 | L2 | L3 | L4 | 生成模式 | 历史状态 |
|---|---|---|---|---|---|---|---|---|---|
| [2026-03-21](./cognitive-portrait-2026-03-21-v0.md) | v0 | 1565 | 999 | ✅ | ✅ | ✅ | ✅ | 2 sub-agent (手工) | ✅ |
| [2026-04-08](./cognitive-portrait-2026-04-08-v2.md) | v2 PoC | 3919 | 1364 | 261 | 284 | 330 | 338 | 4 sub-agent (PoC dispatcher) | ✅ |
| [2026-04-09](./cognitive-portrait-2026-04-09-v3.md) | v3 | 3919 | 1418 | 338 | 326 | 265 | 304 | 4 sub-agent (skill auto dispatcher) | ✅ |
| [2026-04-27](./cognitive-portrait-2026-04-27-v3.md) | v3 | 7239 | 685 | ✅ | ✅ | ✅ | ✅ | 4 sub-agent (skill auto dispatcher) | ✅ |
| [2026-05-27](./cognitive-portrait-2026-05-27-v3.md) | v3 | 8758 | 810 | 173 | 139 | 161 | 290 | 4 sub-agent (compressed L1-L3 after timeout) | ⚠️ partial |
| [2026-05-28](./cognitive-portrait-2026-05-28-v3.md) | v3 修正版 | 8758 | 491 | ✅ | ✅ | ✅ | ✅ | 04-27 narrative style rewrite from same data | ✅ |
| [2026-05-31](./cognitive-portrait-2026-05-31-v3.md) | v3 Codex 试跑 | 8777 | 845 | ✅ | ✅ | ✅ | ✅ | Codex-native skill + collector/validator | ✅ |
| [2026-06-01](./cognitive-portrait-2026-06-01-v3.md) | v3 Codex 增强 | 8777 | 858 | ✅ | ✅ | ✅ | ✅ | Codex skill + 7d delta/evidence collector | ✅ |
| [2026-06-02](./cognitive-portrait-2026-06-02-v3.md) | v3 Codex threads | 8777 | 1343 | 284 | 278 | 390 | 284 | Codex dispatcher + 4 threads | ✅ |
| [2026-07-26](./cognitive-portrait-2026-07-26-v3.md) | v3 Codex threads | 17929 | 1726 | 398 | 422 | 416 | 378 | Codex dispatcher + 4 threads | ✅ |
| [2026-08-09](./cognitive-portrait-2026-08-09-v3.md) | v3 Codex agents | 18642 | 1315 | 303 | 318 | 301 | 331 | Codex dispatcher + 4 agents (3+1 staged) | ✅ |

## 关键趋势

| 指标 | 03-21 → 04-08 | 变化 |
|---|---|---|
| Sessions | 1565 → 3919 | **+150.4%** |
| Expert 率 | ~2.9% → 8.8% | **+5.9pp** ✅ |
| 中高阶合计 | — → 91.7% | 全面跨过 Competent |
| 探索率 | 11% → 6.5% | **-4.5pp** ❌ |
| Delegation | 47% → 49% | +2pp ⚠️ |
| Pair programming | ~3% → 3.0% | 0pp 零进展 |
| 战略广度信号灯 | 🟡 → 🔴 | 退化 |
| 协作效能信号灯 | 🟡 → 🔴 | 退化 |

## 系统演进

| 版本 | 状态 | 生成模式 | 行数能力 | 备注 |
|---|---|---|---|---|
| v0 | archived | 2 sub-agent 手工 | ~978 | 早期实验，jsonl 落盘需重组 |
| v1 | deprecated | 单 agent 单线程 | 419 | 输出衰减，不达 600 行下限 |
| v2 PoC | archived | 4 sub-agent + 手工 dispatcher | 1364 | 验证 multi-agent 假设 |
| v3 | archived | 4 sub-agent + ad-hoc SQL | variable | 历史报告保留，停止新增 |
| v4 | current | deterministic bundle + closed claim catalog + 1 agent + evidence gate | evidence-based | 默认 current/previous 90d；DEGRADED 由 host 阻断 |
| v5 | spec | v4 + 处方追踪 + 自我演进 | evidence-based | Phase 4 |

## Phase 路线图

- **Phase 1（已完成）**: 止血 + 建归档 — v2 PoC 跑通，1364 行 ✅
- **Phase 2（已归档）**: v3 并行画像保留为历史实现，不再新增
- **Phase 3（已落地 2026-07-23）**: launchd 自动化 — 运行
  `scripts/install-local.sh --cognitive-portrait` 显式启用；安装器管理
  `~/Library/LaunchAgents/com.lifcc.refine-cognitive-portrait.plist`。每周日 10:00
  触发 `scripts/cognitive-portrait.sh`，脚本按最新产物日期做 13 天节流 ⇒ 实际双周一份；
  agent 缺失或未产出新画像时 error 日志 + 通知失败 + 非零退出。
  已于 2026-07-23 `launchctl load` 生效。agent 以 `--sandbox workspace-write` 运行，但
  writable workspace 仅为每次运行的随机 staging 目录；归档、INDEX、trusted bundle 和
  validator 均由 host wrapper 独占。Codex 运行同时忽略用户配置/规则并使用 ephemeral
  session。plist 使用安装时发现的 agent 绝对路径及其
  runtime PATH。升级 agent runtime 后重新运行安装命令即可刷新。
- **Phase 3.1（当前）**: v4 固定 cutoff 证据 bundle、closed claim catalog + evidence quality gate，见 [SPEC.md](./SPEC.md)。catalog version、stable ordering 和 canonical rendered lines 是事实的唯一来源；推断必须绑定证据，active Markdown 与超限输入 fail closed；DEGRADED 不启动 agent、不发布报告、不更新本索引。
- **Phase 4（长期）**: 处方追踪 + 指标自校准 + 自我演进

## 命名约定

```
cognitive-portrait-{YYYY-MM-DD}-v4.md
evidence/cognitive-portrait-{YYYY-MM-DD}-v4.bundle.json
evidence/cognitive-portrait-{YYYY-MM-DD}-v4.quality.json
```

- `{YYYY-MM-DD}`: 报告生成日期
- `v4`: 当前 skill/collector 合约版本；历史 v0-v3 文件名保持不变
- `bundle.json`: 同一 cutoff/read snapshot 的可追溯证据
- `quality.json`: factual traceability、unsupported number、可比性、novelty 和 action verifiability 门禁结果

## 不要做的事

- ❌ 不在桌面 (~/Desktop) 留报告 — 全部归档到本目录
- ❌ 不删除旧版本报告 — 历史是长期趋势的一部分
- ❌ 不修改已归档报告 — 如发现错误，新建一份带说明的修订版
- ❌ 不在 INDEX.md 写报告内容 — INDEX 只是索引
