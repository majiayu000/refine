---
name: cognitive-portrait
version: v3 (multi-agent parallel)
status: spec
date: 2026-04-08
author: refine project
---

# Cognitive Portrait v3 — Multi-Agent 并行架构 SPEC

## 1. 问题陈述

### 1.1 v1/v2 的缺陷（事实）

| 版本 | 模式 | 实测产出 | skill 自定下限 | 达标 |
|---|---|---|---|---|
| v1（2026-04-08 原版）| 单 agent 单线程 | 419 行 | 600 行 | ❌ 70% |
| v0（2026-03-21 派 sub-agent）| 2 sub-agent 并行（手工）| 978 行 | — | ✅ |
| v2（2026-04-08 重跑）| 单 agent + 注入约束 | 预测 < 600 行 | 600 行 | 预测 ❌ |

### 1.2 根因分析（推断，置信度：高）

| 现象 | 推断 | 依据 |
|---|---|---|
| 单 agent 写 L4 时只产出 ~70 行（vs 03-21 的 347 行）| **单 agent 上下文累积导致输出衰减** | 同 skill 同模型，唯一变量是上下文长度 |
| 4 sub-agent 并行模式产出 ~978 行 | **每个 sub-agent 在自己 100% context 内全力写** | 03-21 实证 |
| skill v1 输出衰减比例 ~57% (978 → 419) | **衰减不是数据问题，是范式问题** | 数据 +150% 但报告 -57% |

## 2. v3 架构

### 2.1 角色拆分

| 角色 | 数量 | 职责 | 文件所有权（W-14）|
|---|---|---|---|
| Dispatcher（主 agent）| 1 | preflight + SQL 数据采集 + 派发 sub-agent + 合并 + 写盘 | 只读 SQL，写 final markdown |
| L1 Sub-agent | 1 | 写认知演进章节（4 子节）| 写 `/tmp/cp_v{N}_l1.md` |
| L2 Sub-agent | 1 | 写战略定位章节（4 子节）| 写 `/tmp/cp_v{N}_l2.md` |
| L3 Sub-agent | 1 | 写工作方式健康度章节（4 子节）| 写 `/tmp/cp_v{N}_l3.md` |
| L4 Sub-agent | 1 | 写成长处方章节（6 子节）| 写 `/tmp/cp_v{N}_l4.md` |

### 2.2 数据流

```
[refine.db]
  ↓ (Dispatcher 跑 SQL)
[/tmp/cp_data_*.txt]  ← 8 个数据文件（共享只读）
  ↓ (Dispatcher 派 4 个 Task agent in parallel)
┌──────────┬──────────┬──────────┬──────────┐
│ L1 agent │ L2 agent │ L3 agent │ L4 agent │
└────┬─────┴────┬─────┴────┬─────┴────┬─────┘
     ↓          ↓          ↓          ↓
/tmp/cp_v3_l1 /tmp/cp_v3_l2 /tmp/cp_v3_l3 /tmp/cp_v3_l4
     ↓          ↓          ↓          ↓
     └──────────┴──────────┴──────────┘
                    ↓
            [Dispatcher 合并]
                    ↓
docs/cognitive-portraits/cognitive-portrait-{date}-v3.md
```

### 2.3 强制约束（每个 sub-agent prompt 必须包含）

1. **三分离强制**：`[事实]` / `[推断，置信度：高/中/低]` / `[建议]` 显式标签
2. **精确数值驱动**：所有数字 `N/M(百分比%)` 格式
3. **矩阵化优先**：能用表格的不用列表
4. **行数硬下限**：L1/L2/L3 ≥ 250 行，L4 ≥ 280 行
5. **文件所有权隔离**（W-14）：每个 sub-agent 只写自己的 `/tmp/cp_v3_l{N}.md`，禁止动其他文件
6. **范围隔离**：L1 不写 L2/L3/L4 内容，依次类推
7. **章节标题层级**：H2 (`## L{N}：...`) + H3 (`### {N}.{M} ...`)，便于 Dispatcher 合并
8. **必带与上一版对比**：每个 layer 必须引用 03-21 数据做时间序列对比

## 3. PoC 实证（2026-04-08 v2 跑出来）

### 3.1 实测数据

| Sub-agent | 数据来源 | 目标行数 | 实测行数 | 耗时 | 达标 |
|---|---|---|---|---|---|
| L1（认知演进）| 8 个数据文件 | ≥ 250 | **261** | ~260s | ✅ |
| L2（战略定位）| 8 个数据文件 | ≥ 250 | **284** | ~228s | ✅ |
| L3（工作方式）| 8 个数据文件 | ≥ 250 | **330** | ~240s | ✅ |
| L4（成长处方）| 8 个数据文件 | ≥ 280 | **338** | ~279s | ✅ |
| **Sub-agent 合计** | — | ≥ 1030 | **1213** | 4× 并行 | ✅ |
| **Dispatcher 合并** | 4 个 layer + header/footer | ≥ 800 | **1364** | <5s | ✅ |

### 3.2 v3 vs v1 对比（实测）

| 指标 | v1（单 agent）| v3 PoC（multi-agent）| 改善 |
|---|---|---|---|
| 总行数 | 419 | **1364** | **+225%** |
| L4 行数 | ~70 | **338** | **+383%** |
| 三分离严格度 | 弱 | 强（强制标签）| 质变 |
| 处方四件套 | ❌ | ✅（强制结构）| 质变 |
| 决策树 | ❌ | ✅ | 质变 |
| 风险兜底 | ❌ | ✅ | 质变 |
| 单次 token 消耗 | ~30K | ~120K（4×）| 4× 成本 |
| 单次美元成本 | ~$0.2 | ~$0.5-1 | 4× 成本 |

**结论（推断，置信度：高）**：v3 比 v1 内容质量提升 ~3×，成本提升 ~4×。每行成本从 v1 的 $0.5/行 降到 v3 的 ~$0.5-0.9/行（因为单 agent 输出衰减导致每行成本反而高）。

## 4. 实施步骤

### 4.1 修改 SKILL.md

| 段落 | 现状 | v3 改造 |
|---|---|---|
| Step 1 数据采集 | Dispatcher 直接跑 SQL | 不变 |
| Step 2 五层分析 | Dispatcher 单线程写所有内容 | **改为派 4 个 sub-agent 并行 + 合并** |
| Step 3 输出 | 单文件写盘 | 改为 4 临时文件 + 合并写盘 |

### 4.2 新增 prompt 模板

- `~/.claude/skills/cognitive-portrait/prompts/l1_cognitive_evolution.md`
- `~/.claude/skills/cognitive-portrait/prompts/l2_strategic_positioning.md`
- `~/.claude/skills/cognitive-portrait/prompts/l3_work_health.md`
- `~/.claude/skills/cognitive-portrait/prompts/l4_growth_prescription.md`

每个模板包含：数据源列表 + 强制约束 + 章节大纲 + 风格参考 + 输出路径。

### 4.3 合并阶段

Dispatcher 在 4 个 sub-agent 全部完成后：

1. 用 Read 加载 4 个 layer 文件
2. 检查每个 layer 行数是否达标
3. 风格统一 pass：扫描术语/置信度标注/表格格式
4. 拼接：header + 总判断 + L1 + L2 + L3 + L4 + 数据快照
5. 写到 `docs/cognitive-portraits/cognitive-portrait-{date}-v3.md`
6. 更新 `docs/cognitive-portraits/INDEX.md`

## 5. 验证与回归

### 5.1 验收标准

- [ ] 总行数 ≥ 800（理想 950-1100）
- [ ] L1/L2/L3 各 ≥ 250 行 / L4 ≥ 280 行
- [ ] 三分离标签覆盖率 ≥ 80% 段落
- [ ] 处方四件套：5 条处方全部带触发/行动/验证/完成
- [ ] 决策树存在
- [ ] 风险兜底段存在
- [ ] 与 03-21 对比表存在
- [ ] 数据数字与 SQL 输出一致（无幻觉）

### 5.2 回归测试

每次跑完 v3 后，自动追加到 INDEX.md：

```
| date | sessions | total_lines | L1 | L2 | L3 | L4 | 三分离覆盖率 | 是否达标 |
```

连续 4 次达标 → 进入 Phase 3（自动化）。

## 6. 风险

| 风险 | 概率 | 影响 | 兜底 |
|---|---|---|---|
| sub-agent 风格不一致 | 中 | 报告读起来割裂 | Dispatcher 合并阶段做风格统一 pass |
| 4× token 成本 | 高 | $0.5-1/次 | dry-run 模式默认开启 + 失败重试上限 3 |
| sub-agent 超时 | 低 | 个别 layer 缺失 | 失败 layer 单独重跑，不影响其他 |
| 主 agent 合并时上下文超限 | 低 | 合并失败 | sub-agent 写临时文件，主 agent Read 按需加载 |
| sub-agent 写错文件 | 低 | W-14 违规 | prompt 强制声明文件所有权 + Dispatcher 验证文件存在 |

## 7. 不做的事

- ❌ 不做向后兼容（v1/v2 直接废弃）
- ❌ 不引入新依赖（继续用 sqlite3 + claude code Task tool）
- ❌ 不在 v3 阶段做自动化（那是 Phase 3）
- ❌ 不在 v3 阶段做 SQLite 历史表（那是 Phase 4）
- ❌ 不修改 refine.db 表结构

## 7.1 架构硬约束：禁止嵌套 sub-agent 执行

**2026-04-09 验证发现**：Claude Code 的 general-purpose sub-agent **不能嵌套派发 Task 工具**。

实证：派一个 sub-agent 让它跑 cognitive-portrait v3（充当 Dispatcher + 再派 4 个 L1-L4 sub-agent）→ sub-agent 报错 `Task 工具在当前上下文中不可用` → Step 2 直接崩溃。

**结论**：

| 执行姿势 | 可行 |
|---|---|
| 用户 → 主对话（top-level Claude Code）→ 派 4 个 sub-agent | ✅ |
| 用户 → 主对话 → sub-agent → 再派 4 个 sub-sub-agent | ❌ |
| harness / ralph / team / 外部编排 → 调 skill | ❌（除非编排本身是 top-level Claude Code） |
| launchd → headless Claude Code → 调 skill | ❓ 未验证，Phase 3 需要先确认 headless 模式是否支持 Task 工具 |

**Phase 3 自动化的前置实验**：在 launchd headless 模式下跑一次 `claude-code -p "认知画像"`，验证 Task 工具是否可用。如果不可用，Phase 3 的架构需要改（可能要改成 shell 脚本调度 + `claude-code -p` 跑 4 次单 layer，而不是一次完整 skill）。

## 8. 下一阶段（Phase 3 预告）

v3 连续稳定 ≥ 2 周后，进入 Phase 3：
- launchd 周自动运行（参考 weekly-insights 的 plist）
- 失败告警 + 重试
- 自动 INDEX 更新
- "本周变化亮点" 自动产出 + macOS 通知
