# L4 Sub-agent: 成长处方

## File Ownership (W-14)

**You own ONLY: `/tmp/cp_v{N}_l4.md`**
Do NOT create, read, or modify any other file except the 8 read-only data files listed below.

## Data Sources (read-only)

- `/tmp/cp_data_1.txt` — Recent observations overview (last 90 days)
- `/tmp/cp_data_2.txt` — Decision patterns
- `/tmp/cp_data_3.txt` — Bug fix patterns
- `/tmp/cp_data_4.txt` — Knowledge and patterns acquired
- `/tmp/cp_data_5.txt` — Friction and collaboration mode
- `/tmp/cp_data_6.txt` — Project distribution
- `/tmp/cp_data_7.txt` — Cognitive level distribution over time
- `/tmp/cp_data_8.txt` — Recent session insights documents

## Mandatory Constraints

1. **三分离强制**: Every claim must carry one of these labels:
   - `[事实]` — directly traceable to a data file line
   - `[推断，置信度：高/中/低]` — logical inference from facts
   - `[建议]` — action recommendation with prerequisite assumption stated
2. **精确数值**: All numbers in `N/M(百分比%)` format where applicable
3. **矩阵化优先**: Use tables over bullet lists when 3+ items can be compared
4. **行数硬下限**: This file MUST be ≥ 280 lines when complete
5. **与 03-21 对比**: Include at least one comparison table referencing 2026-03-21 baseline data
6. **范围隔离**: Write ONLY L4 content (成长处方). Do NOT write L1, L2, or L3 content.
7. **处方四件套**: Every prescription must include all 4 components:
   - 触发条件 (trigger): when to apply this prescription
   - 行动步骤 (action): concrete 1-3 step sequence
   - 验证方法 (verification): how to know it worked
   - 完成定义 (done-when): objective measurable completion criteria

## Output Path

Write your complete analysis to: `/tmp/cp_v{N}_l4.md`

## Section Outline

Your output must follow this exact structure (H2 + H3 hierarchy, 6 subsections):

```
## L4：成长处方

### 4.1 核心处方清单（5 条）

[Exactly 5 prescriptions, each with 处方四件套]
[Table: 处方 × 触发条件 × 行动步骤 × 验证方法 × 完成定义 × 优先级]
[Each prescription grounded in evidence from data files]

### 4.2 决策树：下一步行动选择

[A markdown decision tree (nested bullet or ASCII) for choosing between prescriptions]
[Branches: current signal light status → prescription to activate]
[Must handle: all-green, mixed, all-red scenarios]

### 4.3 90 天成长路线图

[Table: week_range × focus_area × milestone × success_metric]
[Grounded in current baseline from data]
[Comparison: where this trajectory should land vs 2026-03-21 baseline]

### 4.4 风险兜底方案

[Table: risk × probability × impact × mitigation × fallback]
[At least 5 risks identified]
[Each mitigation must be actionable, not vague]

### 4.5 与上版本对比（时间序列）

[Mandatory comparison table: metric × 2026-03-21 v0 × current × delta × trend]
[At least 8 metrics compared]
[Inferences about whether growth is on track]

### 4.6 执行优先级矩阵

[2×2 matrix: impact (high/low) × effort (high/low)]
[Place all 5 prescriptions and 3+ other actions in the matrix]
[Recommendations: which quadrant to tackle first and why]
[Suggestions with prerequisite assumptions and alternatives]
```

## Style Reference

Minimum density targets:
- Section 4.1: ≥ 70 lines (5 prescriptions × 4 components each + table)
- Section 4.2: ≥ 40 lines (decision tree with all branches)
- Section 4.3: ≥ 40 lines (90-day roadmap table)
- Section 4.4: ≥ 45 lines (risk table with mitigations)
- Section 4.5: ≥ 40 lines (comparison table with 8+ metrics)
- Section 4.6: ≥ 45 lines (priority matrix + reasoning)

This is the most important section — it must be actionable and evidence-grounded.
Every suggestion must state its prerequisite assumption and offer at least one alternative.
