# L1 Sub-agent: 认知演进分析

## File Ownership (W-14)

**You own ONLY: `/tmp/cp_v{N}_l1.md`**
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
4. **行数硬下限**: This file MUST be ≥ 250 lines when complete
5. **与 03-21 对比**: Include at least one comparison table referencing 2026-03-21 baseline data
6. **范围隔离**: Write ONLY L1 content (认知演进). Do NOT write L2 (战略定位), L3 (工作方式), or L4 (成长处方) content.

## Output Path

Write your complete analysis to: `/tmp/cp_v{N}_l1.md`

## Section Outline

Your output must follow this exact structure (H2 + H3 hierarchy):

```
## L1：认知演进

### 1.1 认知等级时序分析

[Time series table: month × cognitive_level × count from data_7.txt]
[Trend analysis with [事实] labels]
[Inference about trajectory with confidence]

### 1.2 决策质量演进

[Analysis of decision patterns from data_2.txt]
[Comparison: current vs 2026-03-21 baseline]
[Table: decision category × frequency × quality signal]

### 1.3 知识积累与模式识别

[Analysis from data_4.txt: knowledge types, pattern density]
[Table: knowledge domain × sessions × depth signal]
[Inferences about learning velocity]

### 1.4 认知瓶颈识别

[What is NOT progressing? What cognitive levels are stagnant?]
[Table: bottleneck × evidence × confidence × impact]
[Suggestions with prerequisite assumptions]
```

## Style Reference

Minimum density targets:
- Section 1.1: ≥ 60 lines (time series data + analysis)
- Section 1.2: ≥ 60 lines (decision analysis + comparison table)
- Section 1.3: ≥ 60 lines (knowledge analysis + tables)
- Section 1.4: ≥ 70 lines (bottleneck analysis + suggestions)

Every paragraph must end with a labeled claim or labeled table row.
Do not use vague phrases like "this seems to indicate" — use explicit confidence labels instead.
