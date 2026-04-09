# Cognitive Portrait — 历史归档索引

> 长期认知画像归档目录。每份报告记录一个时间点的全量认知快照。
> 详细架构和演进路线见 [SPEC.md](./SPEC.md)。

## 报告清单

| date | version | sessions | total_lines | L1 | L2 | L3 | L4 | 生成模式 | 达标 |
|---|---|---|---|---|---|---|---|---|---|
| [2026-03-21](./cognitive-portrait-2026-03-21-v0.md) | v0 | 1565 | 999 | ✅ | ✅ | ✅ | ✅ | 2 sub-agent (手工) | ✅ |
| [2026-04-08](./cognitive-portrait-2026-04-08-v2.md) | v2 PoC | 3919 | 1364 | 261 | 284 | 330 | 338 | 4 sub-agent (PoC dispatcher) | ✅ |
| [2026-04-09](./cognitive-portrait-2026-04-09-v3.md) | v3 | 3919 | 1418 | 338 | 326 | 265 | 304 | 4 sub-agent (skill auto dispatcher) | ✅ |

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
| v3 | current | 4 sub-agent + skill 自动 dispatcher | 1418 | Skill 自动 dispatcher 跑通（2026-04-09） |
| v4 | spec | v3 + launchd 自动化 | ≥ 800 | Phase 3 |
| v5 | spec | v4 + 处方追踪 + 自我演进 | ≥ 800 | Phase 4 |

## Phase 路线图

- **Phase 1（已完成）**: 止血 + 建归档 — v2 PoC 跑通，1364 行 ✅
- **Phase 2（进行中）**: 改 SKILL.md 把 PoC 固化为 v3 — 见 [SPEC.md](./SPEC.md) §4
- **Phase 3（待启动）**: launchd 周自动化 — v3 稳定 2 周后启动
- **Phase 4（长期）**: 处方追踪 + 指标自校准 + 自我演进

## 命名约定

```
cognitive-portrait-{YYYY-MM-DD}-v{N}.md
```

- `{YYYY-MM-DD}`: 报告生成日期
- `v{N}`: skill 版本（v0/v1/v2/v3/v4/v5）

## 不要做的事

- ❌ 不在桌面 (~/Desktop) 留报告 — 全部归档到本目录
- ❌ 不删除旧版本报告 — 历史是长期趋势的一部分
- ❌ 不修改已归档报告 — 如发现错误，新建一份带说明的修订版
- ❌ 不在 INDEX.md 写报告内容 — INDEX 只是索引
